//! Point curves turned into splines, checked against the true curve and by turning the pairs made
//! of them.
//!
//! The true curve is the source made at 1e-8 mm (its points are on the curve, its chords within
//! 1e-8 of it), and for an external tooth also the rack that cuts it: every point of a fitted flank
//! or fillet must lie within the tolerance of the rack's surface at the instant the rack passes
//! closest.

use crate::math::det;
use std::f64::consts::{PI, TAU};

use super::basics::old_sampled;
use super::common::{assert_gap, check_outline, dist, lift, sound, Dice, HOSTILE, HOSTILE_COUNTS};
use crate::error::teeth::{TeethFailure, Why, FIT_DISTANCE};
use crate::prism::profile::MAX_BEZIER_CTRL;
use crate::teeth::bspline::{self, MAX_DEGREE};
use crate::teeth::fit::{
    self as lib, bezier_pieces, cubic_beziers, fit, fit_typed, fitted, one_piece_typed,
    profile_pieces, profile_pieces_typed, solve_normal, Datum, Fitted, Spline, CORRECTIONS,
    DEFAULT_DEGREE, MAX_CONTROL_POINTS, MAX_PROFILE_PIECES, ONE_PIECE_LEAST_DEGREE, SOURCE_SHARE,
    WORKING_POINTS,
};
use crate::teeth::involute::{hub_for, spline_pair, External, Fit};
use crate::teeth::outline::{distance, on_circle, rotated, space_section};
use crate::teeth::{
    in_range, Outline, Pair, Part, Piece, Point, Role, MAX_CURVE_POINTS, MAX_LENGTH,
};

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
type Maker = fn(f64) -> crate::Result<Outline>;

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

/// The reviewer's shaft of 200 teeth of module 0.5 (few points on its nearly straight flanks),
/// made at a tolerance.
fn two_hundred_teeth(t: f64) -> crate::Result<Outline> {
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
        let l = det::hypot(v[0], v[1]);
        [v[0] / l, v[1] / l]
    };
    match piece {
        Piece::Arc { start, sweep, .. } => {
            let a = if at_end { start + sweep } else { *start };
            [-det::sin(a) * sweep.signum(), det::cos(a) * sweep.signum()]
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
) -> crate::Result<()> {
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
fn the_source_decides_which_joins_are_smooth() -> crate::Result<()> {
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
fn the_two_flanks_of_a_tooth_are_mirror_images() -> crate::Result<()> {
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
fn an_external_fitted_tooth_is_what_the_rack_leaves() -> crate::Result<()> {
    for (name, e, tolerance) in [
        ("shaft 20", shaft_of(2.0, 20, 0.0), 1e-4),
        ("undercut shaft 7", shaft_of(3.0, 7, 0.0), 1e-4),
        ("splined shaft", shaft(), 1e-3),
        ("shaft 9, undercut, fine", shaft_of(1.0, 9, -0.1), 1e-5),
    ] {
        let rack = Rack::of(&e);
        let outline = fitted(&e.outline(tolerance * 1e-2)?, tolerance)?;
        let window = det::acos(e.root_radius()? / e.tip_radius()?) + 0.2;
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
fn fitted_part(part: &Part, tolerance: f64) -> crate::Result<Part> {
    let splines = fitted(&part.outline, tolerance)?;
    let outline = Outline::new(splines.teeth(), splines.tooth().to_vec(), MEASURING)?;
    Ok(Part {
        outline,
        ..part.clone()
    })
}

fn fitted_pair(pair: &Pair, tolerance: f64) -> crate::Result<Pair> {
    Ok(Pair {
        first: fitted_part(&pair.first, tolerance)?,
        second: fitted_part(&pair.second, tolerance)?,
        ..pair.clone()
    })
}

#[test]
fn a_fitted_shaft_and_hub_keep_their_gap_within_twice_the_tolerance() -> crate::Result<()> {
    let spline = spline_pair(&shaft(), &hub_fit(), SOURCE)?;
    let side = 0.06 * det::cos(30f64.to_radians()) / 2.0;
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
) -> crate::Result<()> {
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
fn the_bezier_segments_of_a_turned_flank_share_their_ends_bit_for_bit() -> crate::Result<()> {
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
fn the_degree_is_a_parameter_and_every_spline_is_its_bezier_segments() -> crate::Result<()> {
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
fn the_least_squares_solver_solves_and_refuses_what_cannot_be_solved() -> crate::Result<()> {
    // A banded system with a known answer.
    let matrix = [
        4.0, 1.0, 0.0, 0.0, 1.0, 5.0, 2.0, 0.0, 0.0, 2.0, 6.0, 1.0, 0.0, 0.0, 1.0, 3.0,
    ];
    let answer = [1.0, -2.0, 0.5, 3.0];
    let right: Vec<f64> = (0..4)
        .map(|i| (0..4).map(|j| matrix[i * 4 + j] * answer[j]).sum())
        .collect();
    let x = lift(solve_normal(4, &matrix, &right))?;
    assert!(
        x.iter().zip(answer).all(|(a, b)| (a - b).abs() < 1e-13),
        "{x:?}"
    );
    // A full one.
    let full = [2.0, 1.0, 1.0, 1.0, 3.0, 1.0, 1.0, 1.0, 4.0];
    let x = lift(solve_normal(3, &full, &[4.0, 5.0, 6.0]))?;
    for i in 0..3 {
        let back: f64 = (0..3).map(|j| full[i * 3 + j] * x[j]).sum();
        assert!((back - [4.0, 5.0, 6.0][i]).abs() < 1e-13);
    }
    // The same equations scaled far down and far up: the kernel's least pivot is taken relative
    // to the largest number on the diagonal, so their size does not decide.
    for scale in [1e-12, 1e12] {
        let scaled: Vec<f64> = matrix.iter().map(|v| v * scale).collect();
        let right: Vec<f64> = right.iter().map(|v| v * scale).collect();
        let x = lift(solve_normal(4, &scaled, &right))?;
        assert!(
            x.iter().zip(answer).all(|(a, b)| (a - b).abs() < 1e-12),
            "{scale}: {x:?}"
        );
    }
    // What cannot be solved: a matrix that does not fix the unknowns, one that is not positive,
    // sizes that do not agree, numbers that are not finite.
    let not_solvable = Err(TeethFailure::Invalid(Why::FitNotSolvable));
    let not_square = Err(TeethFailure::Invalid(Why::SystemNotSquare));
    assert_eq!(
        solve_normal(2, &[1.0, 1.0, 1.0, 1.0], &[1.0, 1.0]),
        not_solvable
    );
    assert_eq!(
        solve_normal(2, &[-1.0, 0.0, 0.0, 1.0], &[1.0, 1.0]),
        not_solvable
    );
    assert_eq!(
        solve_normal(2, &[1.0, 0.0, 0.0, -1.0], &[1.0, 1.0]),
        not_solvable
    );
    assert_eq!(
        solve_normal(3, &[1.0, 0.0, 0.0, 1.0], &[1.0, 1.0]),
        not_square
    );
    assert_eq!(solve_normal(2, &[1.0, 0.0, 0.0, 1.0], &[1.0]), not_square);
    assert_eq!(
        solve_normal(2, &[1.0, f64::NAN, 0.0, 1.0], &[1.0, 1.0]),
        not_solvable
    );
    assert_eq!(solve_normal(2, &[0.0; 4], &[1.0, 1.0]), not_solvable);
    assert_eq!(solve_normal(usize::MAX, &[], &[]), not_square);
    assert_eq!(solve_normal(0, &[], &[]), Ok(Vec::new()));
    Ok(())
}

#[test]
fn what_cannot_be_fitted_is_refused() -> crate::Result<()> {
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
                (10.0 + 0.3 * det::sin(150.0 * t)) * det::cos(t),
                (10.0 + 0.3 * det::sin(150.0 * t)) * det::sin(t),
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
/// degree, is fitted from a source made at `SOURCE_SHARE` of the tolerance, finest tolerance
/// first: once one succeeds, every coarser one must. Every fit that succeeds claims no more than
/// the tolerance and no less than the true error, measured by the test against the curve made at
/// 1e-8. The reviewer's 200-tooth shaft (few points on its nearly straight flanks) was refused at
/// 1e-3 with degree 2 and at 1e-4 and 1e-5 with degrees 2, 3 and 5, and at 1e-1 with degree 9,
/// while 1e-6 succeeded.
#[test]
fn a_coarser_tolerance_never_fails_where_a_finer_one_succeeds() -> crate::Result<()> {
    let fine_shaft: Maker = two_hundred_teeth;
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
    // The same tolerance from a source made at SOURCE_SHARE and from one made at 1e-8: both met.
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
fn lens(smooth_top: bool, smooth_bottom: bool) -> crate::Result<Outline> {
    const R: f64 = 10.0;
    // Chords within 1e-7 of the circle: their sag is the radius times an eighth of the square of
    // their angle.
    let step = (8.0 * 1e-7 / R).sqrt();
    let n = (0.5 / step).ceil() as usize;
    let lower: Vec<Point> = (0..=n)
        .map(|k| {
            let a = -0.5 + 0.5 * k as f64 / n as f64;
            [R * det::cos(a), R * det::sin(a)]
        })
        .collect();
    let upper: Vec<Point> = lower.iter().rev().map(|&[x, y]| [x, -y]).collect();
    let (top, bottom) = (*upper.last().unwrap(), lower[0]);
    let leaving = rotated(
        [-det::sin(0.5f64), det::cos(0.5f64)],
        if smooth_top { 0.0 } else { 0.3 },
    );
    let arriving = rotated(
        [det::sin(0.5f64), det::cos(0.5f64)],
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
fn smooth_joins_stay_smooth_whether_or_not_a_piece_is_a_mirror_image() -> crate::Result<()> {
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
fn the_fit_is_the_same_on_every_run() -> crate::Result<()> {
    for (_, make) in shapes() {
        let source = make(1e-6)?;
        assert_eq!(fit(&source, 1e-4, 3)?, fit(&source, 1e-4, 3)?);
    }
    Ok(())
}

#[test]
fn hostile_numbers_never_panic() -> crate::Result<()> {
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
            det::hypot(a[0], a[1]),
            det::atan2(b[1], b[0]),
            det::atan2(a[1], a[0]) + PI / 2.0,
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
        if let Ok(x) = solve_normal(size, &matrix, &right) {
            assert!(x.iter().all(|v| v.is_finite()));
        }
        let tolerance = dice.pick(&HOSTILE);
        if let Ok(pieces) = profile_pieces(degree, &knots, &control, tolerance) {
            assert!(pieces.iter().flatten().flatten().all(|v| v.is_finite()));
            assert!(pieces.iter().all(|p| p.len() <= MAX_BEZIER_CTRL));
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
        let accepted = match e.outline(tolerance * SOURCE_SHARE) {
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
// The fit as it was before the proved distance and the decided construction (steps 2, 5 and 6),
// copied letter for letter as the oracle the new fit is counted against. Only the names of what
// moved or changed are new: errors as `TeethFailure`, the old share of the source, the old measure
// as free functions over the fit's own `Spline`, and today's `sampled` as `old_sampled`.
// -------------------------------------------------------------------------------------------------

/// The share of the tolerance the old fit allowed the source.
const OLD_SOURCE_SHARE: f64 = 0.1;

/// The old fit's spline, without the proved bound.
struct OldFit {
    knots: Vec<f64>,
    control: Vec<Point>,
    /// For each of the (cleaned) points, the parameter from which its foot on the spline is found.
    starts: Vec<f64>,
}

/// The old `Spline::distance_from`: the largest distance between the spline and the run of
/// `points` (at parameters `u`), searched, with the parameter where it is.
fn old_distance_from(spline: &Spline, points: &[Point], u: &[f64]) -> (f64, f64) {
    let mut worst = (0.0_f64, 0.0_f64);
    old_measure(spline, points, u, true, &mut |d: f64, at: f64| {
        if !(d <= worst.0) {
            worst = (if d.is_nan() { f64::INFINITY } else { d }, at);
        }
    });
    worst
}

/// The old `Spline::measure`, with its `searched`.
fn old_measure(
    spline: &Spline,
    points: &[Point],
    u: &[f64],
    searched: bool,
    note: &mut dyn FnMut(f64, f64),
) {
    let feet: Vec<(f64, f64)> = points
        .iter()
        .zip(u)
        .map(|(&q, &u)| spline.foot(q, u))
        .collect();
    for &(d, at) in &feet {
        note(d, at);
    }
    for (k, (pair, feet_pair)) in points.windows(2).zip(feet.windows(2)).enumerate() {
        let ([a, b], [(_, fa), (_, fb)]) = (pair, feet_pair) else {
            continue;
        };
        let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let between = 0.5 * (fa + fb);
        note(spline.foot(middle, between).0, between);
        // The spline between the feet, to this chord or a neighbouring one: at each knot
        // between the feet (where the spline may bend), and on each stretch between them at
        // evenly spaced places, the largest of which is then closed in on.
        let to_chords = |t: f64| {
            let s = spline.at(t);
            let mut off = lib::off_chord(s, *a, *b);
            for j in [k.checked_sub(1), Some(k + 1)].into_iter().flatten() {
                if let (Some(&c), Some(&e)) = (points.get(j), points.get(j + 1)) {
                    off = off.min(lib::off_chord(s, c, e));
                }
            }
            off
        };
        note(to_chords(between), between);
        let (low, high) = (fa.min(*fb), fa.max(*fb));
        let inside = spline.knots.partition_point(|&v| v <= low)
            ..spline.knots.partition_point(|&v| v < high);
        let mut previous = low;
        for v in spline
            .knots
            .get(inside)
            .unwrap_or(&[])
            .iter()
            .copied()
            .chain([high])
        {
            if v > previous {
                let (t, off) = if searched {
                    old_largest_between(&to_chords, previous, v)
                } else {
                    (0.5 * (previous + v), to_chords(0.5 * (previous + v)))
                };
                note(off, t);
                if v < high {
                    note(to_chords(v), v);
                }
                previous = v;
            }
        }
    }
}

/// The old `largest_between`: the largest of `f` between `from` and `to` (not at the ends), and
/// where: `f` at evenly spaced places, then closed in on about the largest of them by golden
/// sections.
fn old_largest_between(f: &dyn Fn(f64) -> f64, from: f64, to: f64) -> (f64, f64) {
    const PLACES: usize = 8;
    const SECTIONS: usize = 12;
    let step = (to - from) / PLACES as f64;
    let mut best = (from + 0.5 * (to - from), f(from + 0.5 * (to - from)));
    for j in 1..PLACES {
        let t = from + step * j as f64;
        let value = f(t);
        if !(value <= best.1) {
            best = (t, value);
        }
    }
    let ratio = 0.5 * (5f64.sqrt() - 1.0);
    let (mut a, mut b) = ((best.0 - step).max(from), (best.0 + step).min(to));
    let (mut c, mut d) = (b - ratio * (b - a), a + ratio * (b - a));
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..SECTIONS {
        if fc > fd {
            (b, d, fd) = (d, c, fc);
            c = b - ratio * (b - a);
            fc = f(c);
        } else {
            (a, c, fc) = (c, d, fd);
            d = a + ratio * (b - a);
            fd = f(d);
        }
    }
    for (t, value) in [(c, fc), (d, fd)] {
        if !(value <= best.1) {
            best = (t, value);
        }
    }
    best
}

/// The old `fit`.
fn old_fit(outline: &Outline, tolerance: f64, degree: usize) -> Result<Fitted, TeethFailure> {
    let tolerance = in_range("tolerance", tolerance, 0.0, MAX_LENGTH)?;
    if !(2..=MAX_DEGREE).contains(&degree) {
        return Err(TeethFailure::OutOfRange {
            what: "degree",
            value: degree as f64,
            above: 1.0,
            up_to: MAX_DEGREE as f64,
        });
    }
    let tooth = outline.tooth();
    let source = outline.tolerance();
    let has_curves = tooth
        .iter()
        .any(|piece| matches!(piece, Piece::Curve { .. }));
    if has_curves && !(source <= tolerance * OLD_SOURCE_SHARE) {
        return Err(TeethFailure::Invalid(Why::SourceTooCoarse));
    }
    let size = tooth
        .iter()
        .flat_map(|piece| [piece.start(), piece.end()])
        .flatten()
        .flatten()
        .fold(0.0, |s, v| {
            if v.is_finite() {
                f64::max(s, v.abs())
            } else {
                f64::INFINITY
            }
        });
    if !(size.is_finite() && tolerance >= 1e-10 * (1.0 + size)) {
        return Err(TeethFailure::Invalid(Why::ToleranceTooFineForSize));
    }
    let allowance = tolerance - if has_curves { source } else { 0.0 };
    let joins = lib::join_directions(outline);
    let starting = |i: usize| -> Option<Point> {
        if i == 0 {
            joins
                .last()
                .copied()
                .flatten()
                .map(|d| rotated(d, -outline.pitch_angle()))
        } else {
            joins.get(i - 1).copied().flatten()
        }
    };
    let same = 1e-12 * (1.0 + size);
    let mut pieces: Vec<Piece> = Vec::new();
    let mut within: Vec<Option<f64>> = Vec::new();
    // For each fitted piece, the parameters near which its points' feet on the spline lie.
    let mut feet: Vec<Option<Vec<f64>>> = Vec::new();
    for (i, piece) in tooth.iter().enumerate() {
        let Piece::Curve { points, role } = piece else {
            pieces.push(piece.clone());
            within.push(None);
            feet.push(None);
            continue;
        };
        let ends_of = |k: usize| (starting(k), joins.get(k).copied().flatten());
        let original = tooth.iter().take(i).position(|earlier| match earlier {
            Piece::Curve { points: theirs, .. } => lib::is_mirror_of(points, theirs, same),
            _ => false,
        });
        let original = original.filter(|&k| {
            let (start, end) = ends_of(k);
            let reflected = |d: Option<Point>| d.map(|d| lib::mirror([-d[0], -d[1]]));
            lib::same_direction(ends_of(i).0, reflected(end))
                && lib::same_direction(ends_of(i).1, reflected(start))
        });
        let mirrored = original.and_then(|k| match pieces.get(k) {
            Some(Piece::Spline {
                degree,
                knots,
                control,
                ..
            }) => Some(lib::mirrored_spline(*degree, knots, control)),
            _ => None,
        });
        let own = lib::cleaned(points);
        let (knots, control, starts) = match (
            mirrored,
            original.and_then(|k| feet.get(k).cloned().flatten()),
        ) {
            (Some((knots, control)), Some(theirs)) if theirs.len() == own.len() => (
                knots,
                control,
                theirs.iter().rev().map(|u| 1.0 - u).collect(),
            ),
            _ => {
                let symmetric = lib::is_own_mirror(points, 2.0 * source + 1e-9 * (1.0 + size));
                let ends = (starting(i), joins.get(i).copied().flatten());
                let spline = old_fit_curve(points, allowance, degree, ends, symmetric)?;
                (spline.knots, spline.control, spline.starts)
            }
        };
        let curve = Spline::new(degree, knots, control)?;
        let measured = old_distance_from(&curve, &own, &starts).0;
        feet.push(Some(starts));
        if !(measured <= allowance) {
            return Err(TeethFailure::Invalid(Why::SplineNotWithin));
        }
        within.push(Some(measured + source));
        pieces.push(Piece::Spline {
            degree,
            knots: curve.knots,
            control: curve.control,
            role: *role,
        });
    }
    Ok(Fitted {
        outline: Outline::new_typed(outline.teeth(), pieces, tolerance)?,
        within,
        source_tolerance: source,
    })
}

/// The old `fit_curve`, with its second run at the pace of the chords.
fn old_fit_curve(
    points: &[Point],
    allowance: f64,
    degree: usize,
    directions: (Option<Point>, Option<Point>),
    symmetric: bool,
) -> Result<OldFit, TeethFailure> {
    old_fit_curve_moving(
        points,
        allowance,
        degree,
        directions,
        symmetric,
        CORRECTIONS,
    )
    .or_else(|_| old_fit_curve_moving(points, allowance, degree, directions, symmetric, 0))
}

/// The old `fit_curve_moving`, with its recoveries.
fn old_fit_curve_moving(
    points: &[Point],
    allowance: f64,
    degree: usize,
    directions: (Option<Point>, Option<Point>),
    symmetric: bool,
    corrections: usize,
) -> Result<OldFit, TeethFailure> {
    const TOO_MANY: TeethFailure = TeethFailure::Invalid(Why::TooManyControlPoints);
    let points = lib::cleaned(points);
    if points.len() < 2 || points.len() > MAX_CURVE_POINTS {
        return Err(TeethFailure::Invalid(Why::CurvePointCount));
    }
    let u = lib::parameters(&points)?;
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
        return Err(TeethFailure::Invalid(Why::CurvePointCount));
    };
    let shares: Vec<f64> = (0..points.len())
        .map(|k| {
            let before = if k > 0 {
                lib::cell(&u, k) - lib::cell(&u, k - 1)
            } else {
                0.0
            };
            let after = if k + 1 < u.len() {
                lib::cell(&u, k + 1) - lib::cell(&u, k)
            } else {
                0.0
            };
            0.5 * (before + after)
        })
        .collect();
    let mut data: Vec<Datum> = points
        .iter()
        .zip(&u)
        .zip(&shares)
        .map(|((&q, &u), &weight)| Datum {
            u,
            q,
            weight,
            chord: u,
        })
        .collect();
    if symmetric {
        let mirrored: Vec<Datum> = data
            .iter()
            .rev()
            .map(|d| Datum {
                u: 1.0 - d.u,
                q: lib::mirror(d.q),
                chord: 1.0 - d.chord,
                ..*d
            })
            .collect();
        data.extend(mirrored);
        data.sort_by(|a, b| a.u.total_cmp(&b.u));
    }
    let by_chords = lib::thinned(&data, WORKING_POINTS);
    let mut working = by_chords.clone();
    let need = degree + 2;
    let own: Vec<Datum> = points
        .iter()
        .zip(&u)
        .map(|(&q, &u)| Datum {
            u,
            q,
            weight: 1.0,
            chord: u,
        })
        .collect();
    let checking = lib::thinned(&own, WORKING_POINTS);
    let (check_points, check_chords): (Vec<Point>, Vec<f64>) =
        checking.iter().map(|d| (d.q, d.u)).unzip();
    let mut check_u = check_chords.clone();

    let both = directions.0.is_some() && directions.1.is_some();
    let start_count = if both {
        (degree + 1).max(4)
    } else {
        degree + 1
    };
    let inner = start_count - (degree + 1);
    let mut knots: Vec<f64> = std::iter::repeat_n(0.0, degree + 1)
        .chain((1..=inner).map(|i| i as f64 / (inner + 1) as f64))
        .chain(std::iter::repeat_n(1.0, degree + 1))
        .collect();
    lib::filled(&mut working, &data, degree, &knots, need);
    for _ in 0..=MAX_CONTROL_POINTS {
        let count = knots.len() - degree - 1;
        let mut solved =
            if let Ok(solved) = lib::solve(degree, &knots, &working, (first, last), directions) {
                solved
            } else {
                working.clone_from(&by_chords);
                lib::filled(&mut working, &data, degree, &knots, need);
                lib::solve(degree, &knots, &working, (first, last), directions)?
            };
        for _ in 0..corrections {
            let Ok(spline) = Spline::new(degree, knots.clone(), solved.0.clone()) else {
                break;
            };
            let mut moved = working.clone();
            for d in &mut moved {
                if d.u > 0.0 && d.u < 1.0 {
                    d.u = spline.foot(d.q, d.u).1.clamp(0.0, 1.0);
                }
            }
            if moved.windows(2).any(|w| matches!(w, [a, b] if b.u < a.u)) {
                break;
            }
            match lib::solve(degree, &knots, &moved, (first, last), directions) {
                Ok(again) => (working, solved) = (moved, again),
                Err(_) => break,
            }
        }
        let (mut control, mut first_along, mut last_along) = solved;
        if symmetric {
            let partners: Vec<Point> = control.iter().rev().map(|&p| lib::mirror(p)).collect();
            for (i, (c, p)) in control.iter_mut().zip(partners).enumerate() {
                if i > 0 && i + 1 < count {
                    *c = [0.5 * (c[0] + p[0]), 0.5 * (c[1] + p[1])];
                }
            }
            if directions.0.is_some() && directions.1.is_some() {
                let along = 0.5 * (first_along + last_along);
                (first_along, last_along) = (along, along);
                if let (Some(d0), Some(d1)) = directions {
                    if let Some(c) = control.get_mut(1) {
                        *c = [first[0] + along * d0[0], first[1] + along * d0[1]];
                    }
                    if let Some(c) = control.get_mut(count.saturating_sub(2)) {
                        *c = [last[0] - along * d1[0], last[1] - along * d1[1]];
                    }
                }
            }
        }
        let spline = Spline::new(degree, knots.clone(), control)?;
        let mut by_span = vec![0.0_f64; count];
        let mut record = |d: f64, at: f64| {
            let d = if d.is_nan() { f64::INFINITY } else { d };
            if let Some(e) = by_span.get_mut(lib::span_of(degree, &knots, count, at)) {
                *e = e.max(d);
            }
        };
        if !(first_along > 0.0) {
            record(f64::INFINITY, 0.0);
        } else if !(last_along > 0.0) {
            record(f64::INFINITY, 1.0);
        } else {
            let mut error = 0.0_f64;
            for (u, &q) in check_u.iter_mut().zip(&check_points) {
                if *u > 0.0 && *u < 1.0 {
                    *u = spline.foot(q, *u).1;
                }
            }
            old_measure(&spline, &check_points, &check_u, false, &mut |d, at| {
                error = if d <= error { error } else { d };
                record(d, at);
            });
            if error <= allowance {
                let starts = lib::carried(&u, &check_chords, &check_u);
                let (full, at) = old_distance_from(&spline, &points, &starts);
                if full <= allowance {
                    return Ok(OldFit {
                        knots,
                        control: spline.control,
                        starts,
                    });
                }
                record(f64::INFINITY, at);
            }
        }
        if count + if symmetric { 2 } else { 1 } > MAX_CONTROL_POINTS {
            return Err(TOO_MANY);
        }
        let span = (degree..count)
            .filter(|&s| {
                lib::cell(&by_span, s) > allowance
                    && lib::knot(&knots, s + 1) > lib::knot(&knots, s)
            })
            .max_by(|&a, &b| {
                let score = |s: usize| {
                    lib::cell(&by_span, s)
                        * (lib::knot(&knots, s + 1) - lib::knot(&knots, s)).sqrt()
                };
                score(a).total_cmp(&score(b))
            })
            .ok_or(TOO_MANY)?;
        let middle = 0.5 * (lib::knot(&knots, span) + lib::knot(&knots, span + 1));
        let mut new = vec![middle];
        if symmetric && (middle - 0.5).abs() > 1e-12 {
            new.push(1.0 - middle);
        }
        for value in new {
            if !(value > 0.0 && value < 1.0) || knots.contains(&value) {
                return Err(TOO_MANY);
            }
            let at = knots.iter().position(|&k| k > value).unwrap_or(knots.len());
            knots = lib::with_value_at(&knots, at, value);
        }
        lib::filled(&mut working, &data, degree, &knots, need);
    }
    Err(TOO_MANY)
}

/// The old `one_piece`, the run sampled at the old share of the tolerance.
fn old_one_piece(run: &[Piece], tolerance: f64) -> Result<(Piece, f64), TeethFailure> {
    const FAR: TeethFailure = TeethFailure::Invalid(Why::RunNotOnePiece);
    const NOT_JOINED: TeethFailure = TeethFailure::Invalid(Why::RunNotJoined);
    let tolerance = in_range("tolerance", tolerance, 0.0, MAX_LENGTH)?;
    let (Some(first), Some(last)) = (run.first(), run.last()) else {
        return Err(TeethFailure::Invalid(Why::RunEmpty));
    };
    let source = tolerance * OLD_SOURCE_SHARE;
    let allowance = tolerance - source;
    let size = run
        .iter()
        .flat_map(|piece| [piece.start(), piece.end()])
        .flatten()
        .flatten()
        .fold(0.0, |s, v| {
            if v.is_finite() {
                f64::max(s, v.abs())
            } else {
                f64::INFINITY
            }
        });
    if !size.is_finite() {
        return Err(TeethFailure::Invalid(Why::RunNotFinite));
    }
    let join = 1e-9 * (1.0 + size);
    let mut points: Vec<Point> = Vec::new();
    for piece in run {
        let start = piece.start().ok_or(NOT_JOINED)?;
        if let Some(&previous) = points.last() {
            if distance(previous, start) > join {
                return Err(NOT_JOINED);
            }
        }
        let own = match piece {
            Piece::Curve { points, .. } => points.clone(),
            Piece::Line { from, to, .. } => {
                let (a, b) = (*from, *to);
                let along = |t: f64| [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                old_sampled(&along, 0.0, 1.0, source)?
            }
            Piece::Arc {
                centre,
                radius,
                start,
                sweep,
                ..
            } => {
                let (centre, radius, start, sweep) = (*centre, *radius, *start, *sweep);
                old_sampled(
                    &|t| on_circle(centre, radius, start + sweep * t),
                    0.0,
                    1.0,
                    source,
                )?
            }
            Piece::Spline {
                degree,
                knots,
                control,
                ..
            } => {
                let spline = bspline::Checked::new(*degree, knots, control)?;
                let at = |t: f64| spline.point(t).unwrap_or([f64::NAN, f64::NAN]);
                let mut out: Vec<Point> = Vec::new();
                for stretch in bspline::breaks_typed(*degree, knots, control)?.windows(2) {
                    if let [from, to] = stretch {
                        let part = old_sampled(&at, *from, *to, source)?;
                        let skip = usize::from(!out.is_empty());
                        out.extend(part.into_iter().skip(skip));
                    }
                }
                out
            }
        };
        let skip = usize::from(!points.is_empty());
        points.extend(own.into_iter().skip(skip));
        if points.len() > MAX_CURVE_POINTS {
            return Err(TeethFailure::Invalid(Why::RunTooLong));
        }
    }
    let points = lib::cleaned(&points);
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(TeethFailure::Invalid(Why::RunNotFinite));
    }
    let (Some(&p0), Some(&p1)) = (points.first(), points.last()) else {
        return Err(FAR);
    };
    let u = lib::parameters(&points)?;
    let shares: Vec<f64> = (0..points.len())
        .map(|k| {
            let before = if k > 0 {
                lib::cell(&u, k) - lib::cell(&u, k - 1)
            } else {
                0.0
            };
            let after = if k + 1 < u.len() {
                lib::cell(&u, k + 1) - lib::cell(&u, k)
            } else {
                0.0
            };
            0.5 * (before + after)
        })
        .collect();
    let data: Vec<Datum> = points
        .iter()
        .zip(&u)
        .zip(&shares)
        .map(|((&q, &u), &weight)| Datum {
            u,
            q,
            weight,
            chord: u,
        })
        .collect();
    let directions = (
        lib::side(first, false).map(|s| s.direction),
        lib::side(last, true).map(|s| s.direction),
    );
    let role = first.role();
    for degree in ONE_PIECE_LEAST_DEGREE..=MAX_DEGREE {
        let knots: Vec<f64> = std::iter::repeat_n(0.0, degree + 1)
            .chain(std::iter::repeat_n(1.0, degree + 1))
            .collect();
        let mut working = data.clone();
        let Ok(mut solved) = lib::solve(degree, &knots, &working, (p0, p1), directions) else {
            continue;
        };
        for _ in 0..CORRECTIONS {
            let Ok(spline) = Spline::new(degree, knots.clone(), solved.0.clone()) else {
                break;
            };
            let mut moved = working.clone();
            for d in &mut moved {
                if d.u > 0.0 && d.u < 1.0 {
                    d.u = spline.foot(d.q, d.u).1.clamp(0.0, 1.0);
                }
            }
            if moved.windows(2).any(|w| matches!(w, [a, b] if b.u < a.u)) {
                break;
            }
            match lib::solve(degree, &knots, &moved, (p0, p1), directions) {
                Ok(again) => (working, solved) = (moved, again),
                Err(_) => break,
            }
        }
        let (control, first_along, last_along) = solved;
        if !(first_along > 0.0 && last_along > 0.0) {
            continue;
        }
        let Ok(spline) = Spline::new(degree, knots, control) else {
            continue;
        };
        let starts: Vec<f64> = working.iter().map(|d| d.u).collect();
        let (measured, _) = old_distance_from(&spline, &points, &starts);
        if measured <= allowance {
            let piece = Piece::Spline {
                degree,
                knots: spline.knots,
                control: spline.control,
                role,
            };
            return Ok((piece, measured + source));
        }
    }
    Err(FAR)
}

// -------------------------------------------------------------------------------------------------
// The corpora of the counting tests (steps 2 and 6)
// -------------------------------------------------------------------------------------------------

/// One fit to count: a name, the source outline, the tolerance asked and the degree.
struct FitCase {
    name: String,
    source: Outline,
    tolerance: f64,
    degree: usize,
}

/// C1: the four shapes and the 200-tooth shaft at five tolerances and four degrees, each from a
/// source made at the share of the tolerance.
fn corpus_shapes(degrees: &[usize]) -> Vec<FitCase> {
    let mut all: Vec<(&str, Maker)> = shapes().to_vec();
    all.push(("200 teeth of module 0.5", two_hundred_teeth));
    let mut cases = Vec::new();
    for (name, make) in all {
        for tolerance in [1e-5, 1e-4, 1e-3, 1e-2, 1e-1] {
            let Ok(source) = make(tolerance * SOURCE_SHARE) else {
                continue;
            };
            for &degree in degrees {
                cases.push(FitCase {
                    name: format!("{name}, {tolerance}, degree {degree}"),
                    source: source.clone(),
                    tolerance,
                    degree,
                });
            }
        }
    }
    cases
}

/// The 40 teeth of `a_sweep_of_plausible_teeth_gives_sound_splines_or_a_refusal`, drawn in the
/// same order, each with its tolerance.
fn sweep_teeth() -> Vec<(External, f64)> {
    let mut dice = Dice(2024);
    (0..40)
        .map(|_| {
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
            (e, tolerance)
        })
        .collect()
}

/// C2: the sweep's teeth at degrees 2, 3 and 5.
fn corpus_sweep() -> Vec<FitCase> {
    let mut cases = Vec::new();
    for (k, (e, tolerance)) in sweep_teeth().into_iter().enumerate() {
        let Ok(source) = e.outline(tolerance * SOURCE_SHARE) else {
            continue;
        };
        for degree in [2, 3, 5] {
            cases.push(FitCase {
                name: format!("sweep tooth {k}, {tolerance}, degree {degree}"),
                source: source.clone(),
                tolerance,
                degree,
            });
        }
    }
    cases
}

/// The shafts the planner fits: those of the Bezier test, and 30 drawn ones.
fn planner_shafts() -> Vec<External> {
    let mut shafts: Vec<External> = [
        (28.0, 20_u32),
        (30.0, 24),
        (40.0, 20),
        (30.0, 20),
        (30.0, 16),
    ]
    .into_iter()
    .map(|(diameter, teeth)| External {
        module: diameter / (f64::from(teeth) + 1.0),
        teeth,
        ..shaft()
    })
    .collect();
    let mut dice = Dice(77);
    for _ in 0..30 {
        shafts.push(External {
            teeth: 6 + (dice.roll() % 55) as u32,
            module: dice.pick(&[0.5, 0.8, 1.0, 1.25, 2.0, 3.0, 4.0]),
            pressure_angle: dice.pick(&[20.0, 30.0, 37.5]),
            shift: dice.pick(&[-0.3, -0.1, 0.0, 0.2, 0.5]),
            ..shaft()
        });
    }
    shafts
}

/// The planner's tolerance for a shaft, and the outlines of the shaft and its hub made at `share`
/// of it, each named.
fn planner_sources(
    k: usize,
    shaft: &External,
    share: f64,
) -> (f64, [(String, crate::Result<Outline>); 2]) {
    let within = (shaft.module * 5e-4).clamp(1e-6, 1e-2);
    let hub = hub_for(shaft, &hub_fit()).and_then(|hub| hub.outline(within * share));
    (
        within,
        [
            (
                format!("planner shaft {k}, {within}"),
                shaft.outline(within * share),
            ),
            (format!("planner hub {k}, {within}"), hub),
        ],
    )
}

/// C3: the planner's fits, each shaft and its hub at the planner's tolerance with the source made
/// at `share` of it, degree 3.
fn corpus_planner(share: f64) -> Vec<FitCase> {
    let mut cases = Vec::new();
    for (k, shaft) in planner_shafts().iter().enumerate() {
        let (within, sources) = planner_sources(k, shaft, share);
        for (name, source) in sources {
            let Ok(source) = source else { continue };
            cases.push(FitCase {
                name,
                source,
                tolerance: within,
                degree: 3,
            });
        }
    }
    cases
}

/// The four odd curves of `hostile_numbers_never_panic`, each closed by an arc.
fn odd_outlines() -> Vec<Outline> {
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
    odd.into_iter()
        .filter_map(|points| {
            let (a, b) = (points[0], *points.last().unwrap());
            let (r, a0, a1) = (
                det::hypot(a[0], a[1]),
                det::atan2(b[1], b[0]),
                det::atan2(a[1], a[0]) + PI / 2.0,
            );
            let tooth = vec![
                Piece::Curve {
                    points,
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
            Outline::new(4, tooth, 1e-6).ok()
        })
        .collect()
}

/// C4: the sources of `hostile_numbers_never_panic` at coarse tolerances and four degrees, and
/// its odd curves.
fn corpus_hostile() -> Vec<FitCase> {
    let mut cases = Vec::new();
    let sources = [
        shaft_of(2.0, 20, 0.0).outline(1e-3),
        hub_for(&shaft(), &hub_fit()).and_then(|hub| hub.outline(1e-3)),
    ];
    for (k, source) in sources.into_iter().enumerate() {
        let Ok(source) = source else { continue };
        for tolerance in [0.04, 0.1, 1.0] {
            for degree in [2, 3, 5, 9] {
                cases.push(FitCase {
                    name: format!("hostile source {k}, {tolerance}, degree {degree}"),
                    source: source.clone(),
                    tolerance,
                    degree,
                });
            }
        }
    }
    for (k, source) in odd_outlines().into_iter().enumerate() {
        for tolerance in [1e-2, 1e-4] {
            cases.push(FitCase {
                name: format!("odd curve {k}, {tolerance}"),
                source: source.clone(),
                tolerance,
                degree: DEFAULT_DEGREE,
            });
        }
    }
    cases
}

/// An arc of radius 10 over one radian as a point curve at the parameters `at` (from 0 to 1),
/// closed by an arc, with the outline's tolerance the largest true sag of its chords rounded up.
fn spaced_arc(at: &[f64]) -> Option<Outline> {
    const R: f64 = 10.0;
    let points: Vec<Point> = at
        .iter()
        .map(|&t| [R * det::cos(t), R * det::sin(t)])
        .collect();
    let sag = at
        .windows(2)
        .map(|w| 2.0 * R * det::powi(det::sin(0.25 * (w[1] - w[0])), 2))
        .fold(0.0, f64::max);
    let tooth = vec![
        Piece::Curve {
            points,
            role: Role::Flank,
        },
        Piece::Arc {
            centre: [0.0, 0.0],
            radius: R,
            start: 1.0,
            sweep: TAU - 1.0,
            role: Role::Root,
        },
    ];
    Outline::new(1, tooth, (sag * (1.0 + 1e-9)).max(1e-12)).ok()
}

/// C5: hostile spacing of a source's points: clustered at one end, alternating gaps of 1e-6 and
/// 1e-2 of the parameter, and only 3, 4 or 5 points.
fn corpus_spacing() -> Vec<FitCase> {
    const CLUSTERED: usize = 2000;
    let clustered: Vec<f64> = (0..=CLUSTERED)
        .map(|i| det::powi(i as f64 / CLUSTERED as f64, 3))
        .collect();
    let mut alternating = vec![0.0];
    let (mut t, mut small) = (0.0, true);
    loop {
        t += if small { 1e-6 } else { 1e-2 };
        small = !small;
        if t >= 1.0 {
            break;
        }
        alternating.push(t);
    }
    alternating.push(1.0);
    let mut spacings = vec![("clustered", clustered), ("alternating", alternating)];
    for n in [3_u32, 4, 5] {
        let few = (0..n).map(|i| f64::from(i) / f64::from(n - 1)).collect();
        spacings.push(("few", few));
    }
    let mut cases = Vec::new();
    for (name, at) in spacings {
        let Some(source) = spaced_arc(&at) else {
            continue;
        };
        for tolerance in [1e-2, 1e-3, 1e-4] {
            for degree in [2, 3, 5, 9] {
                cases.push(FitCase {
                    name: format!("{name} ({} points), {tolerance}, degree {degree}", at.len()),
                    source: source.clone(),
                    tolerance,
                    degree,
                });
            }
        }
    }
    cases
}

/// C6: runs for `fit::one_piece`: a flank and its fillet each way round on three teeth made at
/// 1e-6, and the lens's line into a curve and curve into two lines, each with the tolerances to
/// fit at.
fn corpus_runs() -> Vec<(String, Vec<Piece>, f64)> {
    let mut runs: Vec<(String, Vec<Piece>)> = Vec::new();
    for (name, outline) in [
        ("shaft 20", shaft_of(2.0, 20, 0.0).outline(1e-6)),
        ("splined shaft", shaft().outline(1e-6)),
        ("undercut shaft 7", shaft_of(3.0, 7, 0.0).outline(1e-6)),
    ] {
        let outline = outline.expect("the tooth is made");
        let tooth = outline.tooth();
        for (first, second) in [(Role::Flank, Role::Fillet), (Role::Fillet, Role::Flank)] {
            let at = tooth
                .windows(2)
                .position(|w| w[0].role() == first && w[1].role() == second)
                .expect("the run is in the tooth");
            runs.push((
                format!("{name}: {first:?} into {second:?}"),
                tooth[at..at + 2].to_vec(),
            ));
        }
    }
    let both_smooth = lens(true, true).expect("the lens is made");
    let tooth = both_smooth.tooth();
    runs.push((
        "lens: line into curve".into(),
        vec![tooth[4].clone(), tooth[0].clone()],
    ));
    runs.push(("lens: curve into two lines".into(), tooth[1..4].to_vec()));
    let mut cases = Vec::new();
    for (name, run) in runs {
        for tolerance in [1e-2, 1e-3, 1e-4] {
            cases.push((format!("{name}, {tolerance}"), run.clone(), tolerance));
        }
    }
    cases
}

/// The number of control points of each piece (0 for a piece that is not a spline).
fn control_counts(tooth: &[Piece]) -> Vec<usize> {
    tooth
        .iter()
        .map(|piece| match piece {
            Piece::Spline { control, .. } => control.len(),
            _ => 0,
        })
        .collect()
}

/// Whether a refusal is a proof of the fit's distance that could not be made.
fn is_fit_distance(e: &TeethFailure) -> bool {
    matches!(e, TeethFailure::NotProved { what, .. } if *what == FIT_DISTANCE)
}

// -------------------------------------------------------------------------------------------------
// Step 2: one construction, decided on the data
// -------------------------------------------------------------------------------------------------

/// Every fit of the corpora, run once, with the systems that had no solution counted around it:
/// nothing is solved again after a system fails, and a failed system refuses the fit as one the
/// points do not fix. On the teeth (the shapes, the sweep, the planner's fits, the odd spacings
/// and the runs) no system fails at all: the data decide before each solve. Only a hostile odd
/// curve reaches a system that the data check passed and the solve could not settle to within the
/// rounding of its numbers (the check is necessary, not sufficient), and that is refused.
///
/// This construction replaced three recoveries (a second run without the corrections, a reset to
/// the pace of the chords, a silent stop of the corrections). Counted once against them on these
/// 324 cases, before they were taken out: no fit lost and none changed, and the failed systems
/// refused in the same words either way.
#[test]
fn a_failed_solve_refuses_and_nothing_is_solved_again() {
    let failed_solves = || lib::FAILED_SOLVES.with(std::cell::Cell::get);
    let (mut cases, mut refused_on_the_solve) = (0_usize, Vec::new());
    let mut record = |name: &str, hostile: bool, failed: u64, refusal: Option<TeethFailure>| {
        cases += 1;
        assert!(failed <= 1, "{name}: {failed} systems failed in one fit");
        if failed == 1 {
            assert_eq!(
                refusal,
                Some(TeethFailure::Invalid(Why::FitNotSolvable)),
                "{name}"
            );
            assert!(hostile, "{name}: a system of a tooth's fit had no solution");
            refused_on_the_solve.push(name.to_string());
        }
    };
    let teeth: Vec<FitCase> = corpus_shapes(&[2, 3, 5, 9])
        .into_iter()
        .chain(corpus_sweep())
        .chain(corpus_planner(SOURCE_SHARE))
        .chain(corpus_spacing())
        .collect();
    for (hostile, fits) in [(false, teeth), (true, corpus_hostile())] {
        for case in fits {
            let before = failed_solves();
            let refusal = fit_typed(&case.source, case.tolerance, case.degree).err();
            record(&case.name, hostile, failed_solves() - before, refusal);
        }
    }
    for (name, run, tolerance) in corpus_runs() {
        let before = failed_solves();
        let refusal = one_piece_typed(&run, tolerance).err();
        record(&name, false, failed_solves() - before, refusal);
    }
    assert!(cases > 200, "{cases} cases");
    assert!(
        refused_on_the_solve.len() <= 2,
        "{} of {cases}: {refused_on_the_solve:?}",
        refused_on_the_solve.len()
    );
}

// -------------------------------------------------------------------------------------------------
// Step 6: the proved distance, counted against the old measured one
// -------------------------------------------------------------------------------------------------

/// The counts of the step 6 comparison.
#[derive(Default)]
struct ProofCounts {
    /// The old fit gave a spline, the new one refused.
    new_refused: usize,
    /// Both gave splines, and some piece of the new one has fewer control points.
    fewer: usize,
    /// Both gave splines, with control points that differ.
    differ: usize,
    /// New pieces at the most control points where the old had fewer.
    at_cap: usize,
    /// New refusals because the distance could not be proved (the proof's own refusal, whatever
    /// the old fit did), each named with what the old fit gave.
    unproved: Vec<String>,
    /// Both refused, in different words, each named.
    other_words: Vec<String>,
    cases: usize,
    first_refused: Option<String>,
}

impl ProofCounts {
    fn add(
        &mut self,
        name: &str,
        old: Result<Vec<Piece>, TeethFailure>,
        new: Result<Vec<Piece>, TeethFailure>,
    ) {
        self.cases += 1;
        if let Err(e) = &new {
            let was = match &old {
                Ok(_) => "a spline".to_string(),
                Err(o) => o.to_string(),
            };
            if is_fit_distance(e) {
                self.unproved
                    .push(format!("{name}: {e}; the old fit: {was}"));
            } else if old.is_err() && was != e.to_string() {
                self.other_words
                    .push(format!("{name}: {e}; the old fit: {was}"));
            }
        }
        match (old, new) {
            (Ok(_), Err(e)) => {
                self.new_refused += 1;
                self.first_refused
                    .get_or_insert_with(|| format!("{name}: {e}"));
            }
            (Ok(old), Ok(new)) => {
                let (was, now) = (control_counts(&old), control_counts(&new));
                if now.iter().zip(&was).any(|(n, o)| n < o) {
                    self.fewer += 1;
                }
                self.at_cap += now
                    .iter()
                    .zip(&was)
                    .filter(|&(&n, &o)| n == MAX_CONTROL_POINTS && o < n)
                    .count();
                if old != new {
                    self.differ += 1;
                }
            }
            _ => {}
        }
    }
}

/// The proved fit against the old measured one, on the shapes, the sweep, the planner's fits and
/// the runs, every source made at `SOURCE_SHARE` of the tolerance: the proof never refuses what
/// the old measure accepted, never gives fewer control points, never runs into the cap, never
/// refuses as not proved, and where both refuse they refuse in the same words. Then the planner's
/// real settings: the old pipeline from a source at a tenth against the new from a source at a
/// fortieth.
#[test]
fn the_proved_fit_is_never_looser_than_the_old_one() {
    let mut counts = ProofCounts::default();
    let fits = corpus_shapes(&[2, 3, 5, 9])
        .into_iter()
        .chain(corpus_sweep())
        .chain(corpus_planner(SOURCE_SHARE));
    for case in fits {
        let pieces = |fitted: Fitted| fitted.outline.tooth().to_vec();
        counts.add(
            &case.name,
            old_fit(&case.source, case.tolerance, case.degree).map(pieces),
            fit_typed(&case.source, case.tolerance, case.degree).map(pieces),
        );
    }
    for (name, run, tolerance) in corpus_runs() {
        counts.add(
            &name,
            old_one_piece(&run, tolerance).map(|(piece, _)| vec![piece]),
            one_piece_typed(&run, tolerance).map(|(piece, _)| vec![piece]),
        );
    }
    assert!(counts.cases > 200, "{} cases", counts.cases);
    assert_eq!(
        (
            counts.new_refused,
            counts.fewer,
            counts.at_cap,
            counts.unproved.len(),
            counts.other_words.len()
        ),
        (0, 0, 0, 0, 0),
        "new refused, fewer control points, at the cap, refused as not proved, refused by both \
         in other words; {} differ, of {} cases; first refused: {:?}; not proved: {:?}; other \
         words: {:?}",
        counts.differ,
        counts.cases,
        counts.first_refused,
        counts.unproved,
        counts.other_words
    );
    // The planner's real settings: the old fit from its source at a tenth, the new from its source
    // at a fortieth.
    let (mut planner_refused, mut planner_cases) = (0, 0);
    let mut first = None;
    for (k, shaft) in planner_shafts().iter().enumerate() {
        let (within, old_sources) = planner_sources(k, shaft, OLD_SOURCE_SHARE);
        let (_, new_sources) = planner_sources(k, shaft, SOURCE_SHARE);
        for ((name, old), (_, new)) in old_sources.into_iter().zip(new_sources) {
            let Ok(old) = old else { continue };
            planner_cases += 1;
            let was = old_fit(&old, within, 3);
            // A source the finer share cannot make is a refusal of the new pipeline too.
            let now = new
                .map_err(|e| ToString::to_string(&e))
                .and_then(|new| fit_typed(&new, within, 3).map_err(|e| ToString::to_string(&e)));
            if let (Ok(_), Err(e)) = (&was, &now) {
                planner_refused += 1;
                first.get_or_insert_with(|| format!("{name}: {e}"));
            }
        }
    }
    assert_eq!(
        planner_refused, 0,
        "of {planner_cases} planner fits; first: {first:?}"
    );
}

/// `n` points of a spline, evenly in its parameter.
fn spline_points(degree: usize, knots: &[f64], control: &[Point], n: usize) -> Vec<Point> {
    let spline = bspline::Checked::new(degree, knots, control).expect("a spline");
    let (from, to) = bspline::range_typed(degree, knots, control).expect("a spline");
    (0..=n)
        .map(|i| {
            let t = from + (to - from) * i as f64 / n as f64;
            spline.point(t).expect("a point")
        })
        .collect()
}

/// What a fitted piece claims holds against the true curve: for the shapes at degrees 3 and 5,
/// every `within` is at least the largest distance, both ways, between 50,000 points of the spline
/// and the source made at 1e-8, less that 1e-8.
#[test]
fn the_claim_holds_against_dense_points() -> crate::Result<()> {
    let mut all: Vec<(&str, Maker)> = shapes().to_vec();
    all.push(("200 teeth of module 0.5", two_hundred_teeth));
    let mut checked = 0;
    for (name, make) in all {
        let truth = make(SOURCE)?;
        for degree in [3, 5] {
            for tolerance in [1e-5, 1e-4, 1e-3, 1e-2, 1e-1] {
                let Ok(fitted) = fit(&make(tolerance * SOURCE_SHARE)?, tolerance, degree) else {
                    continue;
                };
                for (i, (piece, claim)) in fitted
                    .outline
                    .tooth()
                    .iter()
                    .zip(&fitted.within)
                    .enumerate()
                {
                    let (
                        Some(claim),
                        Piece::Spline {
                            degree: d,
                            knots,
                            control,
                            ..
                        },
                        Piece::Curve { points, .. },
                    ) = (*claim, piece, &truth.tooth()[i])
                    else {
                        continue;
                    };
                    let along = spline_points(*d, knots, control, 50_000);
                    let found = one_way(points, &along).max(one_way(&along, points)) - SOURCE;
                    assert!(
                        claim >= found - 1e-8,
                        "{name}, degree {degree}, {tolerance}: piece {i} claims {claim} but is \
                         {found} away"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 0, "{checked} pieces checked");
    Ok(())
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
        let (sin, cos) = det::sin_cos(e.pressure_angle.to_radians());
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
            det::hypot(u - c[0] - t * dir[0], v - c[1] - t * dir[1])
        };
        let between = det::hypot(
            u - u.clamp(self.left[0], self.right[0].max(self.left[0])),
            v - self.left[1],
        );
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
        let (sn, cs) = det::sin_cos(phi);
        let u = self.r * phi + (-g[0] * sn + g[1] * cs);
        let v = g[0] * cs + g[1] * sn - self.r;
        let u = u.rem_euclid(self.pitch);
        self.one_tooth(u, v)
            .min(self.one_tooth(u - self.pitch, v))
            .min(self.one_tooth(u + self.pitch, v))
    }

    fn closest(&self, g: Point, window: f64) -> f64 {
        const STEPS: usize = 20_000;
        let middle = det::atan2(g[1], g[0]);
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

/// The fit's certificate is the kernel's certified bound of the spline against its source laid on
/// the spline's parameters, which takes any spline on the parameters 0 to 1 with clamped knots,
/// corners included (a knot inside repeated as many times as the degree): its bound holds the
/// distance between the spline and the polyline both ways, measured here by brute force. A spline
/// on other parameters is refused, not measured: the pairing takes the first point at 0 and the
/// last at 1 as the whole spline.
#[test]
fn the_certificate_bounds_every_spline_on_its_parameters_and_refuses_the_others() {
    let spline = |degree: usize, knots: &[f64]| {
        let control: Vec<Point> = (0..knots.len().saturating_sub(degree + 1))
            .map(|i| [i as f64, 0.25 * (i % 2) as f64])
            .collect();
        Spline::new(degree, knots.to_vec(), control).expect("a spline")
    };
    let covered: &[(usize, &[f64])] = &[
        (3, &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]),
        (3, &[0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0]),
        (2, &[0.0, 0.0, 0.0, 0.25, 0.75, 1.0, 1.0, 1.0]),
        (1, &[0.0, 0.0, 1.0, 1.0]),
        (3, &[0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0]),
        (2, &[0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0]),
        (1, &[0.0, 0.0, 0.5, 1.0, 1.0]),
    ];
    let not_covered: &[(usize, &[f64])] = &[
        (3, &[0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0, 2.0]),
        (3, &[1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]),
    ];
    let points: Vec<Point> = (0..=8).map(|i| [0.5 * f64::from(i), 0.0]).collect();
    let starts: Vec<f64> = (0..=8).map(|i| f64::from(i) / 8.0).collect();
    let to_polyline = |p: Point| {
        points
            .windows(2)
            .map(|w| off_chord(p, w[0], w[1]))
            .fold(f64::INFINITY, f64::min)
    };
    for &(degree, knots) in covered {
        let s = spline(degree, knots);
        let (bound, by_span) = lift(lib::certificate(&s, &points, &starts, 1e-3)).unwrap();
        assert!(bound.is_finite(), "{degree} {knots:?}");
        assert_eq!(by_span.len(), s.control.len());
        let along = spline_points(degree, knots, &s.control, 4_000);
        let from_spline = along
            .iter()
            .map(|&p| to_polyline(p))
            .fold(0.0_f64, f64::max);
        let from_points = points
            .windows(2)
            .flat_map(|w| {
                (0..=100).map(move |j| {
                    let t = f64::from(j) / 100.0;
                    [
                        w[0][0] + t * (w[1][0] - w[0][0]),
                        w[0][1] + t * (w[1][1] - w[0][1]),
                    ]
                })
            })
            .map(|q| {
                along
                    .iter()
                    .map(|&p| dist(p, q))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0_f64, f64::max);
        // The dense points of the spline stand a little apart: their own spacing is the slack.
        let spacing = along
            .windows(2)
            .map(|w| dist(w[0], w[1]))
            .fold(0.0_f64, f64::max);
        assert!(
            bound >= from_spline.max(from_points) - spacing,
            "{degree} {knots:?}: {bound} below {from_spline} or {from_points}"
        );
    }
    for &(degree, knots) in not_covered {
        let refused = lib::certificate(&spline(degree, knots), &points, &starts, 1e-3);
        assert!(
            matches!(
                refused,
                Err(TeethFailure::NotProved { what, reached, .. })
                    if what == FIT_DISTANCE && reached == f64::INFINITY
            ),
            "{degree} {knots:?}: {refused:?}"
        );
    }
}

// -------------------------------------------------------------------------------------------------
// Bezier pieces of a profile: a degree above a profile's lowered within the tolerance
// -------------------------------------------------------------------------------------------------

/// A spline of `degree` with `count` control points on a wave about the x axis, on clamped knots
/// spread evenly over 0 to 1.
fn wave(degree: usize, count: usize) -> (Vec<f64>, Vec<Point>) {
    let spans = count - degree;
    let knots: Vec<f64> = (0..=(count + degree))
        .map(|j| j.saturating_sub(degree).min(spans) as f64 / spans as f64)
        .collect();
    let control: Vec<Point> = (0..count)
        .map(|i| {
            let x = i as f64 / (count - 1) as f64;
            [10.0 * x, det::sin(3.0 * PI * x)]
        })
        .collect();
    (knots, control)
}

/// The largest distance, both ways, between dense points of a spline of any degree the kernel
/// holds (its own evaluation) and dense points of its profile pieces, by brute force, and the
/// slack of that measure: the largest step between two dense points of either.
fn pieces_off_spline(
    degree: usize,
    knots: &[f64],
    control: &[Point],
    pieces: &[Vec<Point>],
) -> (f64, f64) {
    let curve = bspline::kernel_curve(degree, knots, control).unwrap();
    let along: Vec<Point> = (0..=3_000)
        .map(|i| {
            let t = f64::from(i) / 3_000.0;
            let d = curve.derivs(t, crate::nurbs::knots::Side::Left, 0).unwrap();
            [d[0].x, d[0].y]
        })
        .collect();
    let on_pieces: Vec<Vec<Point>> = pieces
        .iter()
        .map(|piece| {
            (0..=60)
                .map(|i| bezier_at(piece, f64::from(i) / 60.0))
                .collect()
        })
        .collect();
    let step = |run: &[Point]| {
        run.windows(2)
            .map(|w| dist(w[0], w[1]))
            .fold(0.0_f64, f64::max)
    };
    let mut slack = step(&along);
    for run in &on_pieces {
        slack = slack.max(step(run));
    }
    let flat: Vec<Point> = on_pieces.into_iter().flatten().collect();
    let one_way = |from: &[Point], onto: &[Point]| {
        from.iter()
            .map(|&p| {
                onto.iter()
                    .map(|&q| dist(p, q))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0_f64, f64::max)
    };
    (one_way(&along, &flat).max(one_way(&flat, &along)), slack)
}

/// A spline whose degree a profile holds comes out as its Bezier pieces, exactly as
/// `bezier_pieces` gives them, whatever the tolerance; one of a higher degree is never refused:
/// its pieces are lowered to the highest degree a profile holds, split where one lowered piece
/// would be farther than the tolerance, follow each other end to end, and stay within the
/// tolerance of the spline.
#[test]
fn profile_pieces_lower_a_high_degree_within_the_tolerance() -> crate::Result<()> {
    let most = MAX_BEZIER_CTRL - 1;
    for degree in [2, 3, most] {
        let (knots, control) = wave(degree, degree + 4);
        assert_eq!(
            profile_pieces(degree, &knots, &control, 1e-3)?,
            bezier_pieces(degree, &knots, &control)?
        );
    }
    for degree in [most + 1, most + 3, crate::nurbs::knots::MAX_DEGREE] {
        for tolerance in [1e-2, 1e-4, 1e-6] {
            let (knots, control) = wave(degree, degree + 3);
            let pieces = profile_pieces(degree, &knots, &control, tolerance)?;
            assert!(!pieces.is_empty());
            assert!(
                pieces.iter().all(|p| p.len() == MAX_BEZIER_CTRL),
                "degree {degree}"
            );
            for w in pieces.windows(2) {
                assert_eq!(w[0].last(), w[1].first(), "degree {degree}, {tolerance}");
            }
            assert_eq!(pieces.first().and_then(|p| p.first()), control.first());
            assert_eq!(pieces.last().and_then(|p| p.last()), control.last());
            // The pieces are compared with the spline as curves, both ways, by dense points of
            // the two, which stand apart by up to the measure's slack.
            let (off, slack) = pieces_off_spline(degree, &knots, &control, &pieces);
            assert!(
                off <= tolerance + slack,
                "degree {degree}, {tolerance}: {off} (slack {slack})"
            );
        }
    }
    Ok(())
}

/// What profile pieces refuse, in typed words: a tolerance that is not a positive length, what is
/// not a spline (a degree past the kernel's highest), a tolerance below what the size of the
/// spline can be held to, and a tolerance no number of pieces reaches within the work allowed.
/// Nothing loops: each ends after at most `MAX_PROFILE_PIECES` pieces.
#[test]
fn profile_pieces_refuse_what_they_cannot_hold_in_typed_words() {
    let degree = MAX_BEZIER_CTRL;
    let (knots, control) = wave(degree, degree + 3);
    for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e9] {
        assert!(
            matches!(
                profile_pieces_typed(degree, &knots, &control, tolerance),
                Err(TeethFailure::OutOfRange { .. })
            ),
            "{tolerance}"
        );
    }
    let too_high = crate::nurbs::knots::MAX_DEGREE + 1;
    let (high_knots, high_control) = wave(too_high, too_high + 2);
    assert_eq!(
        profile_pieces_typed(too_high, &high_knots, &high_control, 1e-3),
        Err(TeethFailure::Invalid(Why::NotASpline))
    );
    assert_eq!(
        profile_pieces_typed(degree, &knots, &control, 1e-300),
        Err(TeethFailure::Invalid(Why::ToleranceTooFineForSize))
    );
    // Just above what the size allows, near the kernel's own rounding of the bound: the pieces
    // reach it or the work runs out and says so, after at most `MAX_PROFILE_PIECES` pieces.
    let refused = profile_pieces_typed(degree, &knots, &control, 2e-11);
    assert!(
        matches!(
            refused,
            Ok(_)
                | Err(TeethFailure::NotProved {
                    what: FIT_DISTANCE,
                    ..
                })
        ),
        "{refused:?}"
    );
    // Hostile splines and tolerances: a typed refusal or pieces of finite numbers, never more
    // control points than a profile holds.
    let mut dice = Dice(0x9F1);
    for _ in 0..300 {
        let degree = dice.pick(&HOSTILE_COUNTS) as usize;
        let knots: Vec<f64> = (0..dice.roll() % 30).map(|_| dice.pick(&HOSTILE)).collect();
        let control: Vec<Point> = (0..dice.roll() % 20)
            .map(|_| [dice.pick(&HOSTILE), dice.pick(&HOSTILE)])
            .collect();
        if let Ok(pieces) = profile_pieces_typed(degree, &knots, &control, dice.pick(&HOSTILE)) {
            assert!(pieces.iter().flatten().flatten().all(|v| v.is_finite()));
            assert!(pieces.iter().all(|p| p.len() <= MAX_BEZIER_CTRL));
            assert!(pieces.len() <= MAX_PROFILE_PIECES);
        }
    }
}
