//! Point curves turned into splines, checked against the true curve and by turning the pairs made
//! of them.
//!
//! The true curve is the source made at 1e-8 mm (its points are on the curve, its chords within
//! 1e-8 of it), and for an external tooth also the rack that cuts it: every point of a fitted flank
//! or fillet must lie within the tolerance of the rack's surface at the instant the rack passes
//! closest.

use std::f64::consts::{PI, TAU};

use super::common::{assert_gap, check_outline, dist, sound, Dice, HOSTILE, HOSTILE_COUNTS};
use crate::teeth::fit::{
    bezier_pieces, cubic_beziers, fit, fitted, solve_symmetric, DEFAULT_DEGREE, MAX_CONTROL_POINTS,
    SOURCE_SHARE,
};
use crate::teeth::involute::{hub_for, spline_pair, External, Fit};
use crate::teeth::outline::{rotated, space_section};
use crate::teeth::{bspline, Error, Outline, Pair, Part, Piece, Point, Role};

fn shaft_of(module: f64, teeth: u32, shift: f64) -> External {
    External {
        module,
        teeth,
        pressure_angle: 20.0,
        addendum: 1.0,
        dedendum: 1.25,
        shift,
        tool_tip_radius: 0.3,
        thinning: 0.0,
    }
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

fn hub_fit() -> Fit {
    Fit {
        side_clearance: 0.06,
        hub_addendum: 0.5,
        tip_clearance: 0.2,
        hub_fillet_radius: 0.3,
        form_clearance: 0.05,
    }
}

/// The four shapes of the task, each made at a tolerance.
type Maker = fn(f64) -> Result<Outline, Error>;

fn shapes() -> [(&'static str, Maker); 4] {
    [
        ("shaft tooth", |t| shaft_of(2.0, 20, 0.0).outline(t)),
        ("splined shaft tooth", |t| shaft().outline(t)),
        ("undercut tooth", |t| shaft_of(3.0, 7, 0.0).outline(t)),
        ("internal space", |t| {
            hub_for(&shaft(), &hub_fit())?.outline(t)
        }),
    ]
}

const SOURCE: f64 = 1e-8;
const TOLERANCES: [f64; 4] = [1e-2, 1e-3, 1e-4, 1e-5];

// -------------------------------------------------------------------------------------------------
// The test's own measures
// -------------------------------------------------------------------------------------------------

fn off_chord(p: Point, a: Point, b: Point) -> f64 {
    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
    let length2 = ex * ex + ey * ey;
    let t = if length2 > 0.0 {
        (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist(p, [a[0] + t * ex, a[1] + t * ey])
}

/// Points along a spline, many to each stretch between knots, and how far their chords may be from
/// it at most (measured at the middle of every chord).
fn dense(degree: usize, knots: &[f64], control: &[Point]) -> (Vec<Point>, f64) {
    const EACH: usize = 1000;
    let breaks = bspline::breaks(degree, knots, control).unwrap();
    let at = |u: f64| bspline::point(degree, knots, control, u).unwrap();
    let mut points = vec![at(breaks[0])];
    let mut sag = 0.0_f64;
    for w in breaks.windows(2) {
        for i in 1..=EACH {
            let (u0, u1) = (
                w[0] + (w[1] - w[0]) * (i - 1) as f64 / EACH as f64,
                w[0] + (w[1] - w[0]) * i as f64 / EACH as f64,
            );
            let (a, b) = (at(u0), at(u1));
            sag = sag.max(off_chord(at(0.5 * (u0 + u1)), a, b));
            points.push(b);
        }
    }
    (points, 1.5 * sag)
}

/// The largest distance from the points of `from` to the chords of `onto`, both runs along the same
/// curve in the same direction: a walk that keeps to the nearest chord.
fn one_way(from: &[Point], onto: &[Point]) -> f64 {
    let chord = |j: usize| off_chord(from[0], onto[j], onto[j + 1]);
    let last = onto.len() - 2;
    let mut j = (0..=last)
        .min_by(|&a, &b| chord(a).total_cmp(&chord(b)))
        .unwrap();
    let mut worst = 0.0_f64;
    for &p in from {
        let d = |j: usize| off_chord(p, onto[j], onto[j + 1]);
        while j < last && d(j + 1) <= d(j) {
            j += 1;
        }
        while j > 0 && d(j - 1) < d(j) {
            j -= 1;
        }
        let best = (j.saturating_sub(3)..=(j + 3).min(last))
            .map(d)
            .fold(f64::INFINITY, f64::min);
        worst = worst.max(best);
    }
    worst
}

/// How far a spline is from the run of points of the true curve, both ways, with the error of the
/// measure added.
fn spline_off_curve(piece: &Piece, source: &[Point]) -> f64 {
    let Piece::Spline {
        degree,
        knots,
        control,
        ..
    } = piece
    else {
        panic!("not a spline: {piece:?}")
    };
    let (along, sag) = dense(*degree, knots, control);
    one_way(source, &along).max(one_way(&along, source)) + sag + SOURCE
}

fn end_direction(piece: &Piece, at_end: bool) -> Point {
    let unit = |v: Point| {
        let l = v[0].hypot(v[1]);
        [v[0] / l, v[1] / l]
    };
    match piece {
        Piece::Arc { start, sweep, .. } => {
            let a = if at_end { start + sweep } else { *start };
            [-a.sin() * sweep.signum(), a.cos() * sweep.signum()]
        }
        Piece::Line { from, to, .. } => unit([to[0] - from[0], to[1] - from[1]]),
        Piece::Spline { control, .. } => {
            let n = control.len();
            if at_end {
                unit([
                    control[n - 1][0] - control[n - 2][0],
                    control[n - 1][1] - control[n - 2][1],
                ])
            } else {
                unit([control[1][0] - control[0][0], control[1][1] - control[0][1]])
            }
        }
        Piece::Curve { points, .. } => {
            let n = points.len();
            if at_end {
                unit([
                    points[n - 1][0] - points[n - 2][0],
                    points[n - 1][1] - points[n - 2][1],
                ])
            } else {
                unit([points[1][0] - points[0][0], points[1][1] - points[0][1]])
            }
        }
    }
}

/// The angle at each join of a tooth: piece `i`'s end against the next piece's start (the last
/// against the next tooth's first).
fn join_angles(outline: &Outline) -> Vec<f64> {
    let tooth = outline.tooth();
    (0..tooth.len())
        .map(|i| {
            let before = end_direction(&tooth[i], true);
            let after = if i + 1 < tooth.len() {
                end_direction(&tooth[i + 1], false)
            } else {
                rotated(end_direction(&tooth[0], false), outline.pitch_angle())
            };
            super::common::angle_between(before, after)
        })
        .collect()
}

fn counts(outline: &Outline) -> (usize, usize) {
    let mut found = (0, 0);
    for piece in outline.tooth() {
        match piece {
            Piece::Curve { points, .. } => found.0 += points.len(),
            Piece::Spline { control, .. } => found.1 += control.len(),
            _ => {}
        }
    }
    found
}

// -------------------------------------------------------------------------------------------------
// Within the tolerance of the true curve, closed, smooth where the source is, mirrored, and few
// control points
// -------------------------------------------------------------------------------------------------

#[test]
fn fitted_splines_stay_within_the_tolerance_of_the_true_curve_and_keep_its_joins(
) -> Result<(), Error> {
    for (name, make) in shapes() {
        let source = make(SOURCE)?;
        // Smooth and corner joins of the true curve, from the source's chords at 1e-8 mm.
        let source_angles = join_angles(&source);
        for tolerance in TOLERANCES {
            let fitted = fit(&source, tolerance, DEFAULT_DEGREE)?;
            let outline = &fitted.outline;
            check_outline(name, outline);
            assert_eq!(outline.teeth(), source.teeth());
            assert_eq!((outline.tolerance() - tolerance).abs(), 0.0);
            assert_eq!(outline.tooth().len(), source.tooth().len(), "{name}");
            for (i, (made, was)) in outline.tooth().iter().zip(source.tooth()).enumerate() {
                assert_eq!(made.role(), was.role());
                match was {
                    Piece::Curve { points, .. } => {
                        // Exactly the ends of the piece it replaces.
                        assert_eq!(
                            made.start(),
                            points.first().copied(),
                            "{name} {tolerance}: start of piece {i}"
                        );
                        assert_eq!(
                            made.end(),
                            points.last().copied(),
                            "{name} {tolerance}: end of piece {i}"
                        );
                        let off = spline_off_curve(made, points);
                        assert!(
                            off <= tolerance,
                            "{name} {tolerance}: piece {i} is {off} from the true curve"
                        );
                        let claimed =
                            fitted.within[i].expect("a fitted piece says how close it is");
                        assert!(
                            claimed <= tolerance && claimed > 0.0,
                            "{name}: claimed {claimed}"
                        );
                        // The fit's own measure and the test's agree on the order of the error.
                        assert!(
                            off < 3.0 * claimed + 1e-7,
                            "{name} {tolerance}: measured {off}, claimed {claimed}"
                        );
                    }
                    kept => {
                        assert_eq!(made, kept, "{name}: arcs and lines are kept as they are");
                        assert!(fitted.within[i].is_none());
                    }
                }
            }
            // Smooth joins stay smooth to rounding; corners stay corners.
            for (i, (&now, &was)) in join_angles(outline).iter().zip(&source_angles).enumerate() {
                if was < 0.005 {
                    assert!(
                        now < 1e-9,
                        "{name} {tolerance}: join {i} was smooth ({was}) and has a corner of {now}"
                    );
                } else {
                    assert!(
                        was > 0.05,
                        "{name}: join {i} is neither plainly smooth nor a corner ({was})"
                    );
                    assert!(
                        now > 0.05,
                        "{name} {tolerance}: the corner at join {i} ({was}) is gone ({now})"
                    );
                }
            }
            // Far fewer control points than chord points at the same tolerance.
            let (chord, _) = counts(&make(tolerance)?);
            let (_, control) = counts(outline);
            assert!(
                control < chord,
                "{name} {tolerance}: {control} control points for {chord} chord points"
            );
            if tolerance <= 1e-4 {
                assert!(
                    3 * control <= chord,
                    "{name} {tolerance}: {control} control points for {chord} chord points"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn the_source_decides_which_joins_are_smooth() -> Result<(), Error> {
    // The joins as the definition has them: a plain tooth runs from root into fillet into flank
    // without a corner and has corners at the tip; an undercut tooth has a corner where the fillet
    // cuts the flank.
    let plain = join_angles(&shaft_of(2.0, 20, 0.0).outline(SOURCE)?);
    let undercut = join_angles(&shaft_of(3.0, 7, 0.0).outline(SOURCE)?);
    // Joins: fillet-flank, flank-tip, tip-flank, flank-fillet, fillet-root, root-next fillet.
    for (angles, fillet_flank_is_corner) in [(plain, false), (undercut, true)] {
        let fitted_at = |i: usize| angles[i];
        assert_eq!(fitted_at(0) > 0.05, fillet_flank_is_corner, "{angles:?}");
        assert_eq!(fitted_at(3) > 0.05, fillet_flank_is_corner, "{angles:?}");
        assert!(
            fitted_at(1) > 0.3
                && fitted_at(2) > 0.3
                && fitted_at(4) < 0.005
                && fitted_at(5) < 0.005
        );
    }
    Ok(())
}

#[test]
fn the_two_flanks_of_a_tooth_are_mirror_images() -> Result<(), Error> {
    for (name, make) in shapes() {
        let outline = fitted(&make(1e-6)?, 1e-4)?;
        let splines: Vec<&Piece> = outline
            .tooth()
            .iter()
            .filter(|p| matches!(p, Piece::Spline { .. }))
            .collect();
        let mirrored = |p: &Piece| match p {
            Piece::Spline {
                degree,
                knots,
                control,
                role,
            } => Piece::Spline {
                degree: *degree,
                knots: knots.iter().rev().map(|k| 1.0 - k).collect(),
                control: control.iter().rev().map(|&[x, y]| [x, -y]).collect(),
                role: *role,
            },
            other => other.clone(),
        };
        // The pieces after the middle are the ones before it mirrored, exactly.
        let n = splines.len();
        assert!(n == 2 || n == 4, "{name}: {n}");
        for k in 0..n / 2 {
            assert_eq!(
                mirrored(splines[k]),
                *splines[n - 1 - k],
                "{name}: spline {k}"
            );
        }
    }
    Ok(())
}

#[test]
fn an_external_fitted_tooth_is_what_the_rack_leaves() -> Result<(), Error> {
    for (name, e, tolerance) in [
        ("shaft 20", shaft_of(2.0, 20, 0.0), 1e-4),
        ("undercut shaft 7", shaft_of(3.0, 7, 0.0), 1e-4),
        ("splined shaft", shaft(), 1e-3),
        ("shaft 9, undercut, fine", shaft_of(1.0, 9, -0.1), 1e-5),
    ] {
        let rack = Rack::of(&e);
        let outline = fitted(&e.outline(tolerance * 1e-2)?, tolerance)?;
        let window = (e.root_radius()? / e.tip_radius()?).acos() + 0.2;
        for piece in outline.tooth() {
            let Piece::Spline {
                degree,
                knots,
                control,
                role,
            } = piece
            else {
                continue;
            };
            assert!(matches!(role, Role::Flank | Role::Fillet));
            for i in 0..=24 {
                let g = bspline::point(*degree, knots, control, f64::from(i) / 24.0)?;
                let closest = rack.closest(g, window);
                assert!(
                    closest.abs() <= tolerance + 1e-7,
                    "{name}: {role:?} at {g:?} is {closest} from the rack"
                );
            }
        }
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Pairs made of fitted outlines
// -------------------------------------------------------------------------------------------------

/// How finely a fitted outline is cut into chords for measuring a gap.
const MEASURING: f64 = 1e-6;

/// A part with its outline fitted at `tolerance` and cut into chords of [`MEASURING`] for the
/// measuring.
fn fitted_part(part: &Part, tolerance: f64) -> Result<Part, Error> {
    let splines = fitted(&part.outline, tolerance)?;
    let outline = Outline::new(splines.teeth(), splines.tooth().to_vec(), MEASURING)?;
    Ok(Part {
        outline,
        ..part.clone()
    })
}

fn fitted_pair(pair: &Pair, tolerance: f64) -> Result<Pair, Error> {
    Ok(Pair {
        first: fitted_part(&pair.first, tolerance)?,
        second: fitted_part(&pair.second, tolerance)?,
        ..pair.clone()
    })
}

#[test]
fn a_fitted_shaft_and_hub_keep_their_gap_within_twice_the_tolerance() -> Result<(), Error> {
    let spline = spline_pair(&shaft(), &hub_fit(), SOURCE)?;
    let side = 0.06 * 30f64.to_radians().cos() / 2.0;
    // Each part may be the tolerance off its true curve, and its chords for measuring off its
    // spline.
    assert_gap(
        "spline",
        &fitted_pair(&spline, 1e-4)?,
        5,
        0.5,
        side,
        2.0 * (1e-4 + MEASURING),
    )?;
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Degrees and Bezier segments
// -------------------------------------------------------------------------------------------------

/// A Bezier segment's point by de Casteljau's steps.
fn bezier_at(points: &[Point], t: f64) -> Point {
    let mut d = points.to_vec();
    for round in 1..d.len() {
        for j in 0..d.len() - round {
            d[j] = [
                (1.0 - t) * d[j][0] + t * d[j + 1][0],
                (1.0 - t) * d[j][1] + t * d[j + 1][1],
            ];
        }
    }
    d[0]
}

/// The Bezier segments of a spline are the spline, stretch by stretch.
fn assert_segments_are_the_spline(
    degree: usize,
    knots: &[f64],
    control: &[Point],
) -> Result<(), Error> {
    let pieces = bezier_pieces(degree, knots, control)?;
    let breaks = bspline::breaks(degree, knots, control)?;
    assert_eq!(pieces.len(), breaks.len() - 1);
    let size = control
        .iter()
        .flatten()
        .fold(1.0_f64, |s, v| s.max(v.abs()));
    for (segment, w) in pieces.iter().zip(breaks.windows(2)) {
        assert_eq!(segment.len(), degree + 1);
        for i in 0..=10 {
            let t = f64::from(i) / 10.0;
            let on_spline = bspline::point(degree, knots, control, w[0] + t * (w[1] - w[0]))?;
            let on_segment = bezier_at(segment, t);
            assert!(
                dist(on_spline, on_segment) < 1e-11 * size,
                "{on_spline:?} {on_segment:?}"
            );
        }
    }
    // One after the other: each segment starts at the very point the one before ends at.
    for w in pieces.windows(2) {
        assert_eq!(*w[0].last().unwrap(), w[1][0]);
    }
    Ok(())
}

#[test]
fn the_bezier_segments_of_a_turned_flank_share_their_ends_bit_for_bit() -> Result<(), Error> {
    // Each segment is worked out on its own, and a program that joins segments into one wall by
    // their shared ends must find them the same point. The spline shaft of 20 teeth on a 28 mm
    // shaft left two of its joints a last digit apart (so did 24 teeth on 30 mm and 20 teeth on 40
    // mm), and each such flank became two faces.
    for (diameter, teeth) in [
        (28.0, 20_u32),
        (30.0, 24),
        (40.0, 20),
        (30.0, 20),
        (30.0, 16),
    ] {
        let module = diameter / (f64::from(teeth) + 1.0);
        let within = module * 5e-4;
        let pair = spline_pair(
            &External {
                module,
                teeth,
                ..shaft()
            },
            &hub_fit(),
            within * SOURCE_SHARE,
        )?;
        let outline = fitted(&pair.first.outline, within)?;
        for k in 0..teeth {
            let at = 2.0 * PI * f64::from(k) / f64::from(teeth);
            let section = space_section(&outline, at, outline.radii().1 + 1.0)?;
            for piece in &section.floor {
                let Piece::Spline {
                    degree,
                    knots,
                    control,
                    ..
                } = piece
                else {
                    continue;
                };
                for w in bezier_pieces(*degree, knots, control)?.windows(2) {
                    let (end, start) = (*w[0].last().unwrap(), w[1][0]);
                    assert_eq!(
                        (end[0].to_bits(), end[1].to_bits()),
                        (start[0].to_bits(), start[1].to_bits()),
                        "{diameter} mm, {teeth} teeth, space {k}"
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn the_degree_is_a_parameter_and_every_spline_is_its_bezier_segments() -> Result<(), Error> {
    let source = shaft_of(2.0, 20, 0.0).outline(1e-7)?;
    for degree in [2, 3, 4, 5, 7] {
        let fitted = fit(&source, 1e-4, degree)?;
        check_outline("degree", &fitted.outline);
        for (piece, was) in fitted.outline.tooth().iter().zip(source.tooth()) {
            let (
                Piece::Spline {
                    degree: d,
                    knots,
                    control,
                    ..
                },
                Piece::Curve { points, .. },
            ) = (piece, was)
            else {
                continue;
            };
            assert_eq!(*d, degree);
            assert!(
                spline_off_curve(piece, points) <= 1e-4 + 1e-7,
                "degree {degree}"
            );
            assert_segments_are_the_spline(degree, knots, control)?;
            if degree == 3 {
                let cubic = cubic_beziers(knots, control)?;
                assert_eq!(cubic.len(), bspline::breaks(3, knots, control)?.len() - 1);
            } else {
                assert!(cubic_beziers(knots, control).is_err());
            }
        }
    }
    // Splines not made by the fit: knots that are not clamped, and an inner knot taken twice.
    let control = vec![
        [0.0, 0.0],
        [1.0, 2.0],
        [3.0, 3.0],
        [4.0, 1.0],
        [6.0, 0.5],
        [7.0, 2.0],
    ];
    assert_segments_are_the_spline(
        3,
        &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0],
        &control,
    )?;
    assert_segments_are_the_spline(
        3,
        &[0.0, 0.0, 0.0, 0.0, 0.4, 0.4, 1.0, 1.0, 1.0, 1.0],
        &control,
    )?;
    assert_segments_are_the_spline(2, &[0.0, 0.0, 0.0, 0.2, 0.5, 0.7, 1.0, 1.0, 1.0], &control)?;
    assert_segments_are_the_spline(
        5,
        &[
            -1.0, -1.0, -0.5, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0,
        ],
        &control,
    )?;
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// The solver, refusals, the same result every run, and hostile numbers
// -------------------------------------------------------------------------------------------------

#[test]
fn the_least_squares_solver_solves_and_refuses_what_cannot_be_solved() -> Result<(), Error> {
    // A banded system with a known answer.
    let matrix = [
        4.0, 1.0, 0.0, 0.0, 1.0, 5.0, 2.0, 0.0, 0.0, 2.0, 6.0, 1.0, 0.0, 0.0, 1.0, 3.0,
    ];
    let answer = [1.0, -2.0, 0.5, 3.0];
    let right: Vec<f64> = (0..4)
        .map(|i| (0..4).map(|j| matrix[i * 4 + j] * answer[j]).sum())
        .collect();
    let x = solve_symmetric(4, &matrix, &right)?;
    assert!(
        x.iter().zip(answer).all(|(a, b)| (a - b).abs() < 1e-13),
        "{x:?}"
    );
    // A full one.
    let full = [2.0, 1.0, 1.0, 1.0, 3.0, 1.0, 1.0, 1.0, 4.0];
    let x = solve_symmetric(3, &full, &[4.0, 5.0, 6.0])?;
    for i in 0..3 {
        let back: f64 = (0..3).map(|j| full[i * 3 + j] * x[j]).sum();
        assert!((back - [4.0, 5.0, 6.0][i]).abs() < 1e-13);
    }
    // What cannot be solved: a matrix that does not fix the unknowns, one that is not positive,
    // sizes that do not agree, numbers that are not finite.
    assert!(solve_symmetric(2, &[1.0, 1.0, 1.0, 1.0], &[1.0, 1.0]).is_err());
    assert!(solve_symmetric(2, &[-1.0, 0.0, 0.0, 1.0], &[1.0, 1.0]).is_err());
    assert!(solve_symmetric(3, &[1.0, 0.0, 0.0, 1.0], &[1.0, 1.0]).is_err());
    assert!(solve_symmetric(2, &[1.0, 0.0, 0.0, 1.0], &[1.0]).is_err());
    assert!(solve_symmetric(2, &[1.0, f64::NAN, 0.0, 1.0], &[1.0, 1.0]).is_err());
    assert!(solve_symmetric(usize::MAX, &[], &[]).is_err());
    assert_eq!(solve_symmetric(0, &[], &[])?, Vec::<f64>::new());
    Ok(())
}

#[test]
fn what_cannot_be_fitted_is_refused() -> Result<(), Error> {
    let coarse = shaft_of(2.0, 20, 0.0).outline(1e-3)?;
    assert!(
        fitted(&coarse, 1e-3).is_err(),
        "a source no finer than the splines is refused"
    );
    assert!(fitted(&coarse, 1e-3 / SOURCE_SHARE * 0.99).is_err());
    assert!(fitted(&coarse, 1e-3 / SOURCE_SHARE).is_ok());
    for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e9] {
        assert!(fitted(&coarse, tolerance).is_err(), "{tolerance}");
    }
    for degree in [0, 1, 10, usize::MAX] {
        assert!(fit(&coarse, 0.1, degree).is_err(), "{degree}");
    }
    // An outline of arcs only is kept whatever its tolerance.
    let outer = Outline::new(
        1,
        vec![Piece::Arc {
            centre: [0.0, 0.0],
            radius: 10.0,
            start: 0.0,
            sweep: TAU,
            role: Role::Tip,
        }],
        0.5,
    )?;
    assert_eq!(
        fitted(&outer, 1e-6)?,
        Outline::new(outer.teeth(), outer.tooth().to_vec(), 1e-6)?
    );
    // A curve that runs back on itself cannot be one spline within the tolerance without a loop.
    let hairpin = Outline::new(
        1,
        vec![
            Piece::Curve {
                points: vec![[10.0, 0.0], [12.0, 0.001], [10.0, 0.002]],
                role: Role::Tip,
            },
            Piece::Arc {
                centre: [0.0, 0.0],
                radius: 10.0,
                start: 0.0002,
                sweep: TAU - 0.0002,
                role: Role::Root,
            },
        ],
        1e-6,
    );
    if let Ok(hairpin) = hairpin {
        assert!(sound(fitted(&hairpin, 1e-4)));
    }
    // A rim with 150 waves on half of it needs more control points than one spline may have.
    let waves: Vec<Point> = (0..=40_000)
        .map(|i| PI * f64::from(i) / 40_000.0)
        .map(|t| {
            [
                (10.0 + 0.3 * (150.0 * t).sin()) * t.cos(),
                (10.0 + 0.3 * (150.0 * t).sin()) * t.sin(),
            ]
        })
        .collect();
    let rim = Outline::new(
        1,
        vec![
            Piece::Curve {
                points: waves,
                role: Role::Tip,
            },
            Piece::Arc {
                centre: [0.0, 0.0],
                radius: 10.0,
                start: PI,
                sweep: PI,
                role: Role::Root,
            },
        ],
        1e-6,
    )?;
    assert!(
        fitted(&rim, 1e-4).is_err(),
        "more than {MAX_CONTROL_POINTS} control points"
    );
    assert!(sound(fitted(&rim, 0.5)));
    Ok(())
}

/// A tolerance that a finer one meets is met too, whatever the source holds. Each shape, at each
/// degree, is fitted from a source made at exactly a tenth of the tolerance, finest tolerance
/// first: once one succeeds, every coarser one must. Every fit that succeeds claims no more than
/// the tolerance and no less than the true error, measured by the test against the curve made at
/// 1e-8. The reviewer's 200-tooth shaft (few points on its nearly straight flanks) was refused at
/// 1e-3 with degree 2 and at 1e-4 and 1e-5 with degrees 2, 3 and 5, and at 1e-1 with degree 9,
/// while 1e-6 succeeded.
#[test]
fn a_coarser_tolerance_never_fails_where_a_finer_one_succeeds() -> Result<(), Error> {
    let fine_shaft = |t: f64| {
        External {
            module: 0.5,
            teeth: 200,
            pressure_angle: 14.5,
            addendum: 1.0,
            dedendum: 1.25,
            shift: 0.3,
            tool_tip_radius: 0.05,
            thinning: 0.0,
        }
        .outline(t)
    };
    let mut all: Vec<(&str, Maker)> = shapes().to_vec();
    all.push(("200 teeth of module 0.5", fine_shaft));
    let (mut met, mut refused) = (0_u32, 0_u32);
    for (name, make) in all {
        let truth = make(SOURCE)?;
        for degree in [2, 3, 5, 9] {
            let mut finer_met: Option<f64> = None;
            for tolerance in [1e-5, 1e-4, 1e-3, 1e-2, 1e-1] {
                let source = make(tolerance * SOURCE_SHARE)?;
                let result = fit(&source, tolerance, degree);
                let fitted = match (result, finer_met) {
                    (Ok(fitted), _) => fitted,
                    (Err(e), Some(finer)) => {
                        panic!("{name}, degree {degree}: {tolerance} refused ({e}) although {finer} was met")
                    }
                    (Err(_), None) => {
                        refused += 1;
                        continue;
                    }
                };
                finer_met.get_or_insert(tolerance);
                met += 1;
                for (i, (piece, claim)) in fitted
                    .outline
                    .tooth()
                    .iter()
                    .zip(&fitted.within)
                    .enumerate()
                {
                    let Some(claim) = *claim else { continue };
                    assert!(
                        claim <= tolerance,
                        "{name}, degree {degree}, {tolerance}: piece {i} claims {claim}"
                    );
                    let Piece::Curve { points, .. } = &truth.tooth()[i] else {
                        panic!("{name}: piece {i}")
                    };
                    let Piece::Spline {
                        degree: d,
                        knots,
                        control,
                        ..
                    } = piece
                    else {
                        panic!("{name}: piece {i}")
                    };
                    // The spline's own points against the true curve, and the true curve's points
                    // against the spline: at least the true error, less the 1e-8 of the true
                    // curve's chords.
                    let (along, _) = dense(*d, knots, control);
                    let found = one_way(points, &along).max(one_way(&along, points)) - SOURCE;
                    assert!(claim >= found - 1e-12, "{name}, degree {degree}, {tolerance}: piece {i} claims {claim} but is {found} away");
                }
            }
            // The tolerances the reviewer found refused are met.
            if name == "200 teeth of module 0.5" {
                let finest = finer_met.unwrap_or(f64::INFINITY);
                assert!(
                    finest <= if degree == 2 { 1e-4 } else { 1e-5 },
                    "{name}, degree {degree}: from {finest}"
                );
            }
        }
    }
    // The same tolerance from a source made at a tenth and from one made at 1e-8: both met.
    for degree in [2, 3, 5, 9] {
        for tolerance in [1e-1, 1e-3, 1e-5] {
            for source in [tolerance * SOURCE_SHARE, SOURCE] {
                let outline = fine_shaft(source)?;
                assert!(
                    fit(&outline, tolerance, degree).is_ok(),
                    "degree {degree}, {tolerance} from {source}"
                );
            }
        }
    }
    assert!(met > 80, "{met} met, {refused} refused");
    Ok(())
}

/// A lens: an arc of a circle as two point curves, a lower half and its exact mirror image (the
/// upper half), closed by three lines. The line leaving the upper half and the line arriving at the
/// lower half each continue the circle smoothly or meet it at a corner of 0.3 radians.
fn lens(smooth_top: bool, smooth_bottom: bool) -> Result<Outline, Error> {
    const R: f64 = 10.0;
    // Chords within 1e-7 of the circle: their sag is the radius times an eighth of the square of
    // their angle.
    let step = (8.0 * 1e-7 / R).sqrt();
    let n = (0.5 / step).ceil() as usize;
    let lower: Vec<Point> = (0..=n)
        .map(|k| {
            let a = -0.5 + 0.5 * k as f64 / n as f64;
            [R * a.cos(), R * a.sin()]
        })
        .collect();
    let upper: Vec<Point> = lower.iter().rev().map(|&[x, y]| [x, -y]).collect();
    let (top, bottom) = (*upper.last().unwrap(), lower[0]);
    let leaving = rotated(
        [-(0.5f64).sin(), (0.5f64).cos()],
        if smooth_top { 0.0 } else { 0.3 },
    );
    let arriving = rotated(
        [(0.5f64).sin(), (0.5f64).cos()],
        if smooth_bottom { 0.0 } else { -0.3 },
    );
    let (q1, q2) = (
        [top[0] + 3.0 * leaving[0], top[1] + 3.0 * leaving[1]],
        [bottom[0] - 3.0 * arriving[0], bottom[1] - 3.0 * arriving[1]],
    );
    let tooth = vec![
        Piece::Curve {
            points: lower,
            role: Role::Flank,
        },
        Piece::Curve {
            points: upper,
            role: Role::Flank,
        },
        Piece::Line {
            from: top,
            to: q1,
            role: Role::Tip,
        },
        Piece::Line {
            from: q1,
            to: q2,
            role: Role::Tip,
        },
        Piece::Line {
            from: q2,
            to: bottom,
            role: Role::Root,
        },
    ];
    Outline::new(1, tooth, 1e-7)
}

/// Smooth joins stay smooth whether or not a piece is fitted as the mirror image of another: the
/// reviewer's lens, whose upper half is the mirror of the lower but joins its line smoothly where
/// the lower half has a corner, and every other arrangement of smooth joins and corners at the two
/// ends. Corners stay corners, and where both ends ask the same the upper half is still the lower
/// half's spline mirrored.
#[test]
fn smooth_joins_stay_smooth_whether_or_not_a_piece_is_a_mirror_image() -> Result<(), Error> {
    for (smooth_top, smooth_bottom) in [(true, false), (false, true), (true, true), (false, false)]
    {
        let name = format!("smooth at the top {smooth_top}, at the bottom {smooth_bottom}");
        let source = lens(smooth_top, smooth_bottom)?;
        check_outline(&name, &source);
        for degree in [2, 3, 5] {
            for tolerance in [1e-2, 1e-4] {
                let fitted = fit(&source, tolerance, degree)?.outline;
                check_outline(&name, &fitted);
                let angles = join_angles(&fitted);
                // The joins: lower half into upper half (smooth), upper half into the leaving line,
                // the two lines, and the arriving line into the lower half.
                let at = format!("{name}, degree {degree}, {tolerance}");
                assert!(
                    angles[0] < 1e-9,
                    "{at}: the middle of the arc has a corner of {}",
                    angles[0]
                );
                for (angle, smooth) in [(angles[1], smooth_top), (angles[4], smooth_bottom)] {
                    if smooth {
                        assert!(angle < 1e-9, "{at}: a smooth join has a corner of {angle}");
                    } else {
                        // A free end leaves in the direction the fit finds, near the source's.
                        assert!(
                            (angle - 0.3).abs() < 0.05,
                            "{at}: a corner of 0.3 became {angle}"
                        );
                    }
                }
                let (Piece::Spline { control: a, .. }, Piece::Spline { control: b, .. }) =
                    (&fitted.tooth()[0], &fitted.tooth()[1])
                else {
                    panic!("{at}: not splines");
                };
                if smooth_top == smooth_bottom {
                    let mirrored = a.len() == b.len()
                        && a.iter()
                            .zip(b.iter().rev())
                            .all(|(p, q)| dist(*p, [q[0], -q[1]]) < 1e-12);
                    assert!(
                        mirrored,
                        "{at}: the halves ask the same of their ends but are not mirror images"
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn the_fit_is_the_same_on_every_run() -> Result<(), Error> {
    for (_, make) in shapes() {
        let source = make(1e-6)?;
        assert_eq!(fit(&source, 1e-4, 3)?, fit(&source, 1e-4, 3)?);
    }
    Ok(())
}

#[test]
fn hostile_numbers_never_panic() -> Result<(), Error> {
    let sources = [
        shaft_of(2.0, 20, 0.0).outline(1e-3)?,
        hub_for(&shaft(), &hub_fit())?.outline(1e-3)?,
    ];
    for source in &sources {
        for tolerance in HOSTILE {
            for degree in HOSTILE_COUNTS {
                let _ = sound(fit(source, tolerance, degree as usize).map(|f| f.outline));
            }
        }
    }
    // Odd curves: straight, with repeated points, of two points only, and one that runs back on
    // itself.
    let odd: [Vec<Point>; 4] = [
        (0..=10)
            .map(|i| [10.0, -1.0 + 0.2 * f64::from(i)])
            .collect(),
        vec![
            [10.0, -1.0],
            [10.0, -1.0],
            [10.2, 0.0],
            [10.2, 0.0],
            [10.0, 1.0],
        ],
        vec![[10.0, -1.0], [10.0, 1.0]],
        vec![
            [10.0, -1.0],
            [11.0, 0.0],
            [9.0, 0.0],
            [11.0, 0.5],
            [10.0, 1.0],
        ],
    ];
    for points in odd {
        let (a, b) = (points[0], *points.last().unwrap());
        let (r, a0, a1) = (
            a[0].hypot(a[1]),
            b[1].atan2(b[0]),
            a[1].atan2(a[0]) + PI / 2.0,
        );
        let tooth = vec![
            Piece::Curve {
                points: points.clone(),
                role: Role::Flank,
            },
            Piece::Arc {
                centre: [0.0, 0.0],
                radius: r,
                start: a0,
                sweep: a1 - a0,
                role: Role::Root,
            },
        ];
        if let Ok(outline) = Outline::new(4, tooth, 1e-6) {
            for tolerance in [1e-2, 1e-4, 1e-5] {
                let _ = sound(fitted(&outline, tolerance));
            }
        }
    }
    // Bezier segments of splines that are not splines.
    let mut dice = Dice(7);
    for _ in 0..400 {
        let degree = dice.pick(&HOSTILE_COUNTS) as usize;
        let count = dice.roll() % 8;
        let knots: Vec<f64> = (0..dice.roll() % 14).map(|_| dice.pick(&HOSTILE)).collect();
        let control: Vec<Point> = (0..count)
            .map(|_| [dice.pick(&HOSTILE), dice.pick(&HOSTILE)])
            .collect();
        if let Ok(pieces) = bezier_pieces(degree, &knots, &control) {
            assert!(pieces.iter().flatten().flatten().all(|v| v.is_finite()));
        }
        let _ = cubic_beziers(&knots, &control);
        let size = dice.roll() % 5;
        let matrix: Vec<f64> = (0..size * size).map(|_| dice.pick(&HOSTILE)).collect();
        let right: Vec<f64> = (0..size).map(|_| dice.pick(&HOSTILE)).collect();
        if let Ok(x) = solve_symmetric(size, &matrix, &right) {
            assert!(x.iter().all(|v| v.is_finite()));
        }
    }
    Ok(())
}

#[test]
fn a_sweep_of_plausible_teeth_gives_sound_splines_or_a_refusal() {
    let mut dice = Dice(2024);
    let (mut made, mut refused) = (0, 0);
    for _ in 0..40 {
        let e = External {
            module: dice.pick(&[0.5, 1.0, 2.0, 4.0]),
            teeth: dice.pick(&[5, 7, 9, 12, 20, 33, 80]),
            pressure_angle: dice.pick(&[14.5, 20.0, 25.0, 30.0, 37.5]),
            addendum: dice.pick(&[0.5, 0.8, 1.0, 1.2]),
            dedendum: dice.pick(&[0.9, 1.25, 1.4]),
            shift: dice.pick(&[-0.3, 0.0, 0.2, 0.5]),
            tool_tip_radius: dice.pick(&[0.0, 0.1, 0.25, 0.38]),
            thinning: dice.pick(&[0.0, 0.02]),
        };
        let tolerance = dice.pick(&[1e-2, 1e-3, 1e-4]);
        let accepted = match e.outline(tolerance * 0.05) {
            Ok(source) => sound(fitted(&source, tolerance)),
            Err(_) => false,
        };
        if accepted {
            made += 1;
        } else {
            refused += 1;
        }
    }
    assert!(made > 20, "made {made}, refused {refused}");
}

// -------------------------------------------------------------------------------------------------
// The rack that cuts an external tooth, as tests/involute.rs has it: the test's own model of the
// tool
// -------------------------------------------------------------------------------------------------

struct Rack {
    r: f64,
    pitch: f64,
    sin: f64,
    cos: f64,
    rho: f64,
    left: Point,
    right: Point,
}

impl Rack {
    fn of(e: &External) -> Rack {
        let (sin, cos) = e.pressure_angle.to_radians().sin_cos();
        let (m, pitch) = (e.module, PI * e.module);
        let thickness = pitch / 2.0 + 2.0 * e.shift * m * sin / cos - e.thinning;
        let (rho, depth) = (e.tool_tip_radius * m, (e.dedendum - e.shift) * m);
        let across = rho - depth;
        let along = thickness / 2.0 + (rho - across * sin) / cos;
        Rack {
            r: m * f64::from(e.teeth) / 2.0,
            pitch,
            sin,
            cos,
            rho,
            left: [along, across],
            right: [pitch - along, across],
        }
    }

    fn one_tooth(&self, u: f64, v: f64) -> f64 {
        let ray = |c: Point, dir: Point| {
            let t = ((u - c[0]) * dir[0] + (v - c[1]) * dir[1]).max(0.0);
            (u - c[0] - t * dir[0]).hypot(v - c[1] - t * dir[1])
        };
        let between =
            (u - u.clamp(self.left[0], self.right[0].max(self.left[0]))).hypot(v - self.left[1]);
        let to_drawn_in = ray(self.left, [-self.sin, self.cos])
            .min(ray(self.right, [self.sin, self.cos]))
            .min(between);
        let inside = (u - self.left[0]) * self.cos + (v - self.left[1]) * self.sin >= 0.0
            && (self.right[0] - u) * self.cos + (v - self.right[1]) * self.sin >= 0.0
            && v >= self.left[1];
        if inside {
            -to_drawn_in - self.rho
        } else {
            to_drawn_in - self.rho
        }
    }

    fn distance(&self, g: Point, phi: f64) -> f64 {
        let (sn, cs) = phi.sin_cos();
        let u = self.r * phi + (-g[0] * sn + g[1] * cs);
        let v = g[0] * cs + g[1] * sn - self.r;
        let u = u.rem_euclid(self.pitch);
        self.one_tooth(u, v)
            .min(self.one_tooth(u - self.pitch, v))
            .min(self.one_tooth(u + self.pitch, v))
    }

    fn closest(&self, g: Point, window: f64) -> f64 {
        const STEPS: usize = 20_000;
        let middle = g[1].atan2(g[0]);
        let at = |i: usize| middle - window + 2.0 * window * (i as f64) / (STEPS as f64);
        let (mut best, mut best_i) = (f64::INFINITY, 0);
        for i in 0..=STEPS {
            let d = self.distance(g, at(i));
            if d < best {
                (best, best_i) = (d, i);
            }
        }
        let (mut lo, mut hi) = (at(best_i.saturating_sub(1)), at((best_i + 1).min(STEPS)));
        for _ in 0..200 {
            let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if self.distance(g, a) < self.distance(g, b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        best.min(self.distance(g, 0.5 * (lo + hi)))
    }
}
