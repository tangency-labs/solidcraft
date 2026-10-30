//! Original analytic hatch-output cases; no external pattern library or CAD output.
use std::f64::consts::FRAC_PI_2;

use super::budget::Limits;
use super::generate::generate_with_limits;
use super::{Family, Mark, Pattern, generate};
use crate::{Loop2, Region2, Seg2, Vec2};

// Comparison only, never an admission/snap tolerance. The rotated and decimal
// fixtures stay within 66 mm: this is under 128 binary64 epsilons at that scale.
// Axis-aligned dyadic fixtures below instead require exact coordinates.
const ROUNDOFF_MM: f64 = 1e-12;

fn family(origin: [f64; 2], dx: f64, dy: f64, dashes: &[f64]) -> Family {
    Family { angle: 0.0, origin: Vec2::new(origin[0], origin[1]), delta_x: dx, delta_y: dy, dashes: dashes.to_vec() }
}

fn pattern(families: Vec<Family>) -> Pattern {
    Pattern { name: "ORIGINAL_PHASE".into(), description: "Independent analytic output cases".into(), families }
}

fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
    Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)])
}

fn region(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
    Region2 { outer: rectangle(x0, y0, x1, y1), holes: vec![] }
}

fn near_point(actual: Vec2, expected: [f64; 2], tolerance: f64) -> bool {
    actual.is_finite() && (actual.x - expected[0]).abs() <= tolerance && (actual.y - expected[1]).abs() <= tolerance
}

fn assert_marks(marks: &[Mark], lines: &[[f64; 4]], dots: &[[f64; 2]], tolerance: f64) {
    // Match the complete multiset, allowing traversal and endpoint direction to
    // differ. Consuming each match separately preserves coincident dot instances.
    assert_eq!(marks.len(), lines.len() + dots.len(), "unexpected marks: {marks:?}");
    let mut unmatched = marks.to_vec();
    for &[ax, ay, bx, by] in lines {
        let found = unmatched.iter().position(|mark| match *mark {
            Mark::Line { a, b } => {
                assert!(a.is_finite() && b.is_finite() && a != b, "invalid stroke: {mark:?}");
                (near_point(a, [ax, ay], tolerance) && near_point(b, [bx, by], tolerance))
                    || (near_point(a, [bx, by], tolerance) && near_point(b, [ax, ay], tolerance))
            }
            Mark::Dot { .. } => false,
        });
        let Some(index) = found else { panic!("missing line {ax},{ay} -> {bx},{by}; remaining: {unmatched:?}") };
        unmatched.swap_remove(index);
    }
    for &at in dots {
        let found = unmatched.iter().position(|mark| match *mark {
            Mark::Dot { at: actual } => near_point(actual, at, tolerance),
            Mark::Line { .. } => false,
        });
        let Some(index) = found else { panic!("missing dot {at:?}; remaining: {unmatched:?}") };
        unmatched.swap_remove(index);
    }
    assert!(unmatched.is_empty(), "unexpected marks: {unmatched:?}");
}

#[test]
fn clipped_first_stroke_keeps_the_origin_and_negative_cycles() {
    // Strokes are [2 + 5*j, 5 + 5*j]. A reset at -5.5 also gives total
    // length 9, so only the individual endpoints distinguish the wrong phase.
    let p = pattern(vec![family([2.0, 0.0], 0.0, 10.0, &[3.0, -2.0])]);
    let r = region(-5.5, -0.5, 9.5, 0.5);
    let marks = generate(&p, &r).unwrap();
    assert_marks(&marks, &[[-5.5, 0.0, -5.0, 0.0], [-3.0, 0.0, 0.0, 0.0], [2.0, 0.0, 5.0, 0.0], [7.0, 0.0, 9.5, 0.0]], &[], 0.0);
}

#[test]
fn tangential_delta_changes_phase_once_for_positive_and_negative_members() {
    // k=-1,0,1 have y=-2,0,2 and x origins -1.5,0,1.5.
    let p = pattern(vec![family([0.0, 0.0], 1.5, 2.0, &[2.0, -3.0])]);
    let r = region(0.25, -2.5, 8.25, 2.5);
    let marks = generate(&p, &r).unwrap();
    assert_marks(
        &marks,
        &[[0.25, -2.0, 0.5, -2.0], [3.5, -2.0, 5.5, -2.0], [0.25, 0.0, 2.0, 0.0], [5.0, 0.0, 7.0, 0.0], [1.5, 2.0, 3.5, 2.0], [6.5, 2.0, 8.25, 2.0]],
        &[],
        0.0,
    );
}

#[test]
fn rotated_negative_deltas_keep_global_origin_and_signed_member_phase() {
    // u=(0,1), n=(-1,0), hence B_k=(4+3*k,1-k), k=-1,0,1,2.
    let mut f = family([4.0, 1.0], -1.0, -3.0, &[1.0, -2.0]);
    f.angle = FRAC_PI_2;
    let marks = generate(&pattern(vec![f]), &region(0.5, -0.5, 10.5, 5.5)).unwrap();
    assert_marks(
        &marks,
        &[
            [1.0, -0.5, 1.0, 0.0],
            [1.0, 2.0, 1.0, 3.0],
            [1.0, 5.0, 1.0, 5.5],
            [4.0, 1.0, 4.0, 2.0],
            [4.0, 4.0, 4.0, 5.0],
            [7.0, 0.0, 7.0, 1.0],
            [7.0, 3.0, 7.0, 4.0],
            [10.0, -0.5, 10.0, 0.0],
            [10.0, 2.0, 10.0, 3.0],
            [10.0, 5.0, 10.0, 5.5],
        ],
        &[],
        ROUNDOFF_MM,
    );
}

#[test]
fn a_hole_splits_a_dash_without_pausing_or_restarting_its_cycle() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[3.0, -2.0])]);
    let mut r = region(-0.5, -0.75, 14.5, 0.75);
    r.holes.push(rectangle(2.5, -0.25, 8.5, 0.25).reversed());
    let marks = generate(&p, &r).unwrap();
    assert_marks(&marks, &[[0.0, 0.0, 2.5, 0.0], [10.0, 0.0, 13.0, 0.0]], &[], 0.0);
}

#[test]
fn explicit_dots_keep_negative_cycles_and_phase_across_a_hole() {
    // Every cycle has stroke [4*j,4*j+1] and dot 4*j+2.
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[1.0, -1.0, 0.0, -2.0])]);
    let mut r = region(-2.5, -0.75, 10.5, 0.75);
    r.holes.push(rectangle(1.5, -0.25, 5.5, 0.25).reversed());
    let marks = generate(&p, &r).unwrap();
    assert_marks(&marks, &[[0.0, 0.0, 1.0, 0.0], [8.0, 0.0, 9.0, 0.0]], &[[-2.0, 0.0], [6.0, 0.0], [10.0, 0.0]], 0.0);
}

#[test]
fn converted_paper_millimetres_and_common_translation_are_applied_once() {
    // Values are already converted by 12.7 mm per authored length. There is
    // no PAT reader or units argument in this generator-level output oracle.
    let p = pattern(vec![family([3.175, 1.5875], 6.35, 12.7, &[3.175, -3.175, 0.0, -6.35])]);
    let marks = generate(&p, &region(0.0, 0.0, 25.4, 3.175)).unwrap();
    assert_marks(&marks, &[[3.175, 1.5875, 6.35, 1.5875], [15.875, 1.5875, 19.05, 1.5875]], &[[9.525, 1.5875], [22.225, 1.5875]], ROUNDOFF_MM);

    let translated = pattern(vec![family([43.175, -5.4125], 6.35, 12.7, &[3.175, -3.175, 0.0, -6.35])]);
    let marks = generate(&translated, &region(40.0, -7.0, 65.4, -3.825)).unwrap();
    assert_marks(
        &marks,
        &[[43.175, -5.4125, 46.35, -5.4125], [55.875, -5.4125, 59.05, -5.4125]],
        &[[49.525, -5.4125], [62.225, -5.4125]],
        ROUNDOFF_MM,
    );
}

#[test]
fn reversing_signed_member_parameterization_preserves_the_same_geometry() {
    let r = region(0.25, -2.5, 8.25, 2.5);
    let p = pattern(vec![family([0.0, 0.0], -1.5, -2.0, &[2.0, -3.0])]);
    let marks = generate(&p, &r).unwrap();
    assert_marks(
        &marks,
        &[[0.25, -2.0, 0.5, -2.0], [3.5, -2.0, 5.5, -2.0], [0.25, 0.0, 2.0, 0.0], [5.0, 0.0, 7.0, 0.0], [1.5, 2.0, 3.5, 2.0], [6.5, 2.0, 8.25, 2.0]],
        &[],
        0.0,
    );
}

#[test]
fn changing_loop_start_and_orientation_does_not_restart_the_pattern() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[3.0, -2.0])]);
    let outer = rectangle(-0.5, -0.75, 14.5, 0.75);
    let hole = rectangle(2.5, -0.25, 8.5, 0.25);
    for reverse_outer in [false, true] {
        for reverse_hole in [false, true] {
            for start in 0..4 {
                let mut r = Region2 {
                    outer: if reverse_outer { outer.reversed() } else { outer.clone() },
                    holes: vec![if reverse_hole { hole.reversed() } else { hole.clone() }],
                };
                r.outer.segs.rotate_left(start);
                r.holes[0].segs.rotate_left((start + 1) % 4);
                let marks = generate(&p, &r).unwrap();
                assert_marks(&marks, &[[0.0, 0.0, 2.5, 0.0], [10.0, 0.0, 13.0, 0.0]], &[], 0.0);
            }
        }
    }
}

#[test]
fn continuous_lines_ignore_tangential_phase_and_keep_holes() {
    let p = pattern(vec![family([7.0, 0.0], 3.5, 2.0, &[])]);
    let mut r = region(-1.5, -2.5, 4.5, 2.5);
    r.holes.push(rectangle(0.5, -0.25, 1.5, 0.25).reversed());
    let marks = generate(&p, &r).unwrap();
    assert_marks(&marks, &[[-1.5, -2.0, 4.5, -2.0], [-1.5, 0.0, 0.5, 0.0], [1.5, 0.0, 4.5, 0.0], [-1.5, 2.0, 4.5, 2.0]], &[], 0.0);
}

#[test]
fn repeated_zero_tokens_are_distinct_even_at_neighboring_cycle_boundaries() {
    let r = region(-0.25, -0.5, 2.25, 0.5);
    let expected = [[0.0, 0.0], [0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [2.0, 0.0], [2.0, 0.0]];
    for dashes in [&[0.0, 0.0, -1.0][..], &[0.0, -1.0, 0.0][..]] {
        let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, dashes)]);
        let marks = generate_with_limits(&p, &r, Limits { marks: 6, ..Limits::default() }).unwrap();
        assert_marks(&marks, &[], &expected, 0.0);
        assert!(generate_with_limits(&p, &r, Limits { marks: 5, ..Limits::default() }).is_err());
    }
}

#[test]
fn dots_on_outer_and_hole_boundaries_are_omitted_but_interior_dots_remain() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[0.0, -1.0])]);
    let mut r = region(0.0, -0.75, 6.0, 0.75);
    r.holes.push(rectangle(2.0, -0.25, 4.0, 0.25).reversed());
    let marks = generate(&p, &r).unwrap();
    assert_marks(&marks, &[], &[[1.0, 0.0], [5.0, 0.0]], 0.0);
}

#[test]
fn a_stroke_retains_boundary_endpoints_without_becoming_a_dot() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[1.0, -1.0])]);
    let marks = generate(&p, &region(0.0, -0.5, 1.0, 0.5)).unwrap();
    assert_marks(&marks, &[[0.0, 0.0, 1.0, 0.0]], &[], 0.0);
    // At x=1 and x=2 the adjacent positive strokes have only point contact.
    let marks = generate(&p, &region(1.0, -0.5, 2.0, 0.5)).unwrap();
    assert_marks(&marks, &[], &[], 0.0);
}

#[test]
fn independent_families_keep_their_own_origins_and_phase() {
    let p = pattern(vec![family([0.0, -0.25], 0.0, 10.0, &[1.0, -1.0]), family([0.5, 0.25], 0.0, 10.0, &[0.0, -2.0])]);
    let marks = generate(&p, &region(-0.25, -0.5, 3.25, 0.5)).unwrap();
    assert_marks(&marks, &[[0.0, -0.25, 1.0, -0.25], [2.0, -0.25, 3.0, -0.25]], &[[0.5, 0.25], [2.5, 0.25]], 0.0);
}

#[test]
fn all_negative_cycles_paint_nothing_instead_of_continuous_lines() {
    let p = pattern(vec![family([0.0, 0.0], 1.5, -2.0, &[-1.0, -2.0])]);
    let marks = generate(&p, &region(-4.5, -2.5, 4.5, 2.5)).unwrap();
    assert_marks(&marks, &[], &[], 0.0);
}

#[test]
fn painting_nothing_does_not_bypass_pattern_or_region_validation() {
    let gap = family([0.0, 0.0], 0.0, 10.0, &[-1.0]);
    let p = pattern(vec![gap.clone()]);
    // Canonical full circles are supported; a different seam remains outside
    // the admitted two-semicircle encoding, even for a gap-only pattern.
    let curved = Region2 { outer: Loop2::circle_from(Vec2::ZERO, 1.0, FRAC_PI_2), holes: vec![] };
    assert!(generate(&p, &curved).is_err());
    assert!(generate(&p, &Region2::default()).is_err());
    let mut open = region(-1.0, -0.5, 1.0, 0.5);
    open.outer.segs.truncate(3);
    assert!(generate(&p, &open).is_err());

    let r = region(-1.0, -0.5, 1.0, 0.5);
    let invalid = Family { delta_y: 0.0, ..gap.clone() };
    assert!(generate(&pattern(vec![gap, invalid]), &r).is_err());
    let mut invalid_name = p;
    invalid_name.name.clear();
    assert!(generate(&invalid_name, &r).is_err());
}

#[test]
fn a_later_invalid_family_refuses_the_entire_result() {
    let good = family([0.0, 0.0], 0.0, 10.0, &[]);
    let r = region(-1.0, -0.5, 1.0, 0.5);
    let mut bad = good.clone();
    bad.angle = f64::NAN;
    assert!(generate(&pattern(vec![good, bad]), &r).is_err());
}

#[test]
fn supported_line_input_does_not_silently_accept_an_unsupported_hole() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[])]);
    let mut r = region(-2.0, -2.0, 2.0, 2.0);
    r.holes.push(Loop2::circle(Vec2::ZERO, 0.5).reversed());
    assert!(generate(&p, &r).is_err());
}

#[test]
fn boundary_runs_are_omitted_without_shifting_the_interior_members() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, 1.0, &[])]);
    let marks = generate(&p, &region(-1.0, 0.0, 1.0, 2.0)).unwrap();
    assert_marks(&marks, &[[-1.0, 1.0, 1.0, 1.0]], &[], 0.0);
}

#[test]
fn each_work_budget_can_refuse_a_small_otherwise_valid_pattern() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[1.0, -1.0])]);
    let r = region(-0.25, -0.5, 1.25, 0.5);
    assert_marks(&generate(&p, &r).unwrap(), &[[0.0, 0.0, 1.0, 0.0]], &[], 0.0);
    for limits in [
        Limits { input_pieces: 0, ..Limits::default() },
        Limits { candidate_lines: 0, ..Limits::default() },
        Limits { work: 0, ..Limits::default() },
        Limits { events: 0, ..Limits::default() },
        Limits { dash_steps: 0, ..Limits::default() },
        Limits { marks: 0, ..Limits::default() },
    ] {
        assert!(generate_with_limits(&p, &r, limits).is_err());
    }
}

#[test]
fn marks_are_counted_across_families_and_split_strokes_without_partial_success() {
    let first = family([0.0, -0.25], 0.0, 10.0, &[]);
    let second = family([0.0, 0.25], 0.0, 10.0, &[]);
    let mut r = region(-1.5, -0.5, 2.5, 0.5);
    r.holes.push(rectangle(0.0, -0.375, 1.0, 0.375).reversed());
    let p = pattern(vec![first, second]);
    let marks = generate_with_limits(&p, &r, Limits { marks: 4, ..Limits::default() }).unwrap();
    assert_marks(&marks, &[[-1.5, -0.25, 0.0, -0.25], [1.0, -0.25, 2.5, -0.25], [-1.5, 0.25, 0.0, 0.25], [1.0, 0.25, 2.5, 0.25]], &[], 0.0);
    assert!(generate_with_limits(&p, &r, Limits { marks: 3, ..Limits::default() }).is_err());
}

#[test]
fn candidate_lines_are_counted_even_without_marks() {
    let r = region(-0.25, -0.5, 0.25, 0.5);
    let f = family([0.0, 0.0], 0.0, 10.0, &[-1.0]);
    let p = pattern(vec![f.clone(), f]);
    assert!(generate(&p, &r).unwrap().is_empty());
    // Each family has member k=0 in the region; a one-line budget cannot
    // serve both. Conservative exterior candidates also consume this budget.
    assert!(generate_with_limits(&p, &r, Limits { candidate_lines: 1, ..Limits::default() }).is_err());
}

#[test]
fn candidate_lines_and_dash_steps_are_charged_for_a_visible_gap() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[1.0, -3.0])]);
    let r = region(1.25, -0.5, 3.75, 0.5);
    assert!(generate(&p, &r).unwrap().is_empty());
    assert!(generate_with_limits(&p, &r, Limits { candidate_lines: 0, ..Limits::default() }).is_err());
    assert!(generate_with_limits(&p, &r, Limits { dash_steps: 0, ..Limits::default() }).is_err());
}

#[test]
fn subnormal_spacing_is_valid_data_but_cannot_expand_without_a_bound() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, f64::from_bits(1), &[])]);
    assert!(p.validate().is_ok());
    assert!(generate(&p, &region(-0.5, -0.5, 0.5, 0.5)).is_err());
}

#[test]
fn a_nonzero_dash_step_that_cannot_advance_is_refused() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[1.0, -f64::from_bits(1)])]);
    assert!(p.validate().is_ok());
    assert!(generate(&p, &region(-0.25, -0.5, 2.25, 0.5)).is_err());
}

#[test]
fn unresolvable_far_phase_indices_are_not_saturated_into_a_success() {
    let p = pattern(vec![family([100_000.0, 0.0], 0.0, 10.0, &[2.0_f64.powi(-40), -2.0_f64.powi(-40)])]);
    let r = region(-2.0_f64.powi(-50), -0.5, 2.0_f64.powi(-50), 0.5);
    assert!(p.validate().is_ok());
    assert!(generate(&p, &r).is_err());
}

#[test]
fn collapsed_or_nonfinite_line_boundaries_return_an_error() {
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[])]);
    for point in [Vec2::ZERO, Vec2::new(f64::NAN, 0.0), Vec2::new(f64::INFINITY, 0.0)] {
        let r = Region2 { outer: Loop2 { segs: vec![Seg2::Line { a: point, b: point }] }, holes: vec![] };
        assert!(generate(&p, &r).is_err());
    }
}

#[test]
fn a_large_member_phase_cancels_before_its_single_output_rounding() {
    // Independent integer oracle: k*dx = 99_999_000_000_099_999.
    // With period 100 and cycle -999_990_000_001_000, the visible stroke
    // is exactly [-1,0]. Rounding k*dx first instead shifts it to [0,1].
    let k: i64 = 1_000_000_000_001;
    let dx: i64 = 99_999;
    let cycle: i128 = -999_990_000_001_000;
    let exact_start = i128::from(k) * i128::from(dx) + cycle * 100;
    assert_eq!(exact_start, -1);
    assert_eq!(exact_start + 1, 0);

    // These dyadic bounds contain only member k and preserve their width
    // exactly. Stored origin, deltas and dashes all satisfy model admission.
    let dy = 2.0_f64.powi(-40);
    let p = pattern(vec![family([0.0, 0.0], dx as f64, dy, &[1.0, -99.0])]);
    let r = region(-2.0, (k as f64 - 0.25) * dy, 2.0, (k as f64 + 0.25) * dy);
    assert!(p.validate().is_ok());
    let y = k as f64 * dy;
    assert_marks(&generate(&p, &r).unwrap(), &[[-1.0, y, 0.0, y]], &[], 0.0);
}

#[test]
fn decimal_dash_cycles_round_complete_endpoints_once() {
    // The exact sum of the stored binary64 values 0.1 and 0.2 is
    // 10_808_639_105_689_191 / 2^55: it lies between representable f64 values.
    // With n=3_602_879_701_896_397, the exact endpoints for j=-2..2 are
    // [-6n,-5n], [-3n,-2n], [0,n], [3n,4n], [6n,7n], all divided by 2^55.
    // In particular -5n/2^55 rounds to -0.5; adding a rounded cycle base
    // to the first dash would instead round that endpoint twice.
    let p = pattern(vec![family([0.0, 0.0], 0.0, 10.0, &[0.1, -0.2])]);
    let r = region(-0.75, -0.5, 0.75, 0.5);
    assert!(p.validate().is_ok());
    assert_marks(
        &generate(&p, &r).unwrap(),
        &[
            [-0.6000000000000001, 0.0, -0.5, 0.0],
            [-0.30000000000000004, 0.0, -0.2, 0.0],
            [0.0, 0.0, 0.1, 0.0],
            [0.30000000000000004, 0.0, 0.4, 0.0],
            [0.6000000000000001, 0.0, 0.7000000000000001, 0.0],
        ],
        &[],
        0.0,
    );
}

#[test]
fn a_large_decimal_cycle_preserves_the_stored_period_residual() {
    // Let n=3_602_879_701_896_397, D=2^55 and K=J=2^50.
    // The stored dx is -(3n+1)/D, whereas the exact dash period is 3n/D.
    // K*dx + J*period = -1/32; adding the first dash gives
    // (n-2^50)/D = 2_476_979_795_053_773/D, exactly the stored 0.06875.
    // Repeating the rounded period would wrongly yield [0,0.1].
    let dy = 2.0_f64.powi(-50);
    let p = pattern(vec![family([0.0, 0.0], -0.30000000000000004, dy, &[0.1, -0.2])]);
    let half_height = 2.0_f64.powi(-52);
    let r = region(-0.0625, 1.0 - half_height, 0.1875, 1.0 + half_height);
    assert!(p.validate().is_ok());
    assert_marks(&generate(&p, &r).unwrap(), &[[-0.03125, 1.0, 0.06875, 1.0]], &[], 0.0);
}

#[test]
fn a_nonrepresentable_dot_phase_rounds_only_after_interior_classification() {
    // Stored dx=3_002_399_751_580_331/2^53; member k=3 gives the exact
    // phase 1+2^-53, enclosed by [1,1+2^-52] and rounded nearest-even to 1.
    // That enclosure is strictly inside (0,2), so its single dot is retained.
    let p = pattern(vec![family([0.0, 0.0], 0.33333333333333337, 1.0, &[0.0, -4.0])]);
    let r = region(0.0, 2.5, 2.0, 3.5);
    assert_marks(&generate(&p, &r).unwrap(), &[], &[[1.0, 3.0]], 0.0);
}

#[test]
fn a_dot_enclosure_overlapping_a_boundary_refuses_the_entire_result() {
    // The same exact phase lies above 1, but its f64 enclosure touches the
    // boundary at 1. Neither dropping its rounded point nor accepting its
    // midpoint certifies membership. A preceding family must not leak output.
    let first = family([0.0, 3.0], 0.0, 10.0, &[]);
    let ambiguous = family([0.0, 0.0], 0.33333333333333337, 1.0, &[0.0, -4.0]);
    let r = region(1.0, 2.5, 2.0, 3.5);
    assert_marks(&generate(&pattern(vec![first.clone()]), &r).unwrap(), &[[1.0, 3.0, 2.0, 3.0]], &[], 0.0);
    assert!(generate(&pattern(vec![ambiguous.clone()]), &r).is_err());
    assert!(generate(&pattern(vec![first, ambiguous]), &r).is_err());
}

fn circular_region(with_hole: bool) -> Region2 {
    let center = Vec2::new(2.0, -1.0);
    Region2 { outer: Loop2::circle(center, 5.0), holes: if with_hole { vec![Loop2::circle(center, 3.0)] } else { vec![] } }
}

fn circle_lines(intervals: &[[f64; 3]]) -> Vec<Mark> {
    // Independent world map of the analytic (left t, right t, relative y)
    // tables for center (2,-1), horizontal tangent and vertical normal.
    intervals.iter().map(|&[left, right, q]| Mark::Line { a: Vec2::new(2.0 + left, -1.0 + q), b: Vec2::new(2.0 + right, -1.0 + q) }).collect()
}

fn circle_dots(q: f64, positions: &[f64]) -> Vec<Mark> {
    positions.iter().map(|&t| Mark::Dot { at: Vec2::new(2.0 + t, -1.0 + q) }).collect()
}

fn circle_output(p: &Pattern, r: &Region2, limits: Option<Limits>) -> Result<Vec<Mark>, super::Invalid> {
    let pattern_before = p.clone();
    let region_before = r.clone();
    let result = match limits {
        Some(limits) => generate_with_limits(p, r, limits),
        None => generate(p, r),
    };
    assert_eq!(p, &pattern_before);
    assert_eq!(r, &region_before);
    result
}

fn assert_exact_circle_output(p: &Pattern, r: &Region2, expected: &[Mark], total_length: f64) {
    let marks = circle_output(p, r, None).unwrap();
    // Full equality checks member/span/cycle order and each repeated zero token.
    assert_eq!(marks, expected);
    let length: f64 = marks
        .iter()
        .map(|mark| match *mark {
            Mark::Line { a, b } => {
                assert!(a.is_finite() && b.is_finite() && a.x < b.x);
                assert_eq!(a.y, b.y);
                b.x - a.x
            }
            Mark::Dot { at } => {
                assert!(at.is_finite());
                0.0
            }
        })
        .sum();
    assert_eq!(length, total_length);
    let exact = Limits { marks: expected.len(), ..Limits::default() };
    assert_eq!(circle_output(p, r, Some(exact)).unwrap(), expected);
    if !expected.is_empty() {
        let short = Limits { marks: expected.len() - 1, ..Limits::default() };
        assert!(circle_output(p, r, Some(short)).is_err());
    }
}

#[test]
fn a_disk_generates_three_ordered_secants_with_total_length_twenty_six() {
    let r = circular_region(false);
    let p = pattern(vec![family([2.0, -1.0], 0.0, 3.0, &[])]);
    // x^2 + q^2 = 25 in centered coordinates: q=-3,0,3 gives A=4,5,4.
    let expected = circle_lines(&[[-4.0, 4.0, -3.0], [-5.0, 5.0, 0.0], [-4.0, 4.0, 3.0]]);
    assert_exact_circle_output(&p, &r, &expected, 26.0);
}

#[test]
fn a_disk_keeps_twenty_three_strict_integer_dots_and_excludes_its_boundary() {
    let r = circular_region(false);
    let p = pattern(vec![family([2.0, -1.0], 0.0, 3.0, &[0.0, -1.0])]);
    let mut expected = circle_dots(-3.0, &[-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0]);
    expected.extend(circle_dots(0.0, &[-4.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0]));
    expected.extend(circle_dots(3.0, &[-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0]));
    assert_eq!(expected.len(), 23);
    assert_exact_circle_output(&p, &r, &expected, 0.0);
}

#[test]
fn outer_circle_tangencies_and_exterior_members_emit_neither_lines_nor_dots() {
    for with_hole in [false, true] {
        let r = circular_region(with_hole);
        for q in [-6.0, -5.0, 5.0, 6.0] {
            for dashes in [&[][..], &[0.0, -1.0][..]] {
                // Only k=0 can meet the circle; q=+/-5 is an isolated contact,
                // whereas q=+/-6 has no roots. Both must return empty success.
                let p = pattern(vec![family([2.0, -1.0 + q], 0.0, 32.0, dashes)]);
                assert_exact_circle_output(&p, &r, &[], 0.0);
            }
        }
    }
}

#[test]
fn an_annulus_keeps_six_ordered_spans_and_splits_both_inner_tangencies() {
    let r = circular_region(true);
    let p = pattern(vec![family([2.0, -1.0], 0.0, 3.0, &[])]);
    let expected = circle_lines(&[[-4.0, 0.0, -3.0], [0.0, 4.0, -3.0], [-5.0, -3.0, 0.0], [3.0, 5.0, 0.0], [-4.0, 0.0, 3.0], [0.0, 4.0, 3.0]]);
    assert_exact_circle_output(&p, &r, &expected, 20.0);
}

#[test]
fn an_annulus_keeps_fourteen_strict_dots_without_a_dot_at_inner_tangency() {
    let r = circular_region(true);
    let p = pattern(vec![family([2.0, -1.0], 0.0, 3.0, &[0.0, -1.0])]);
    let mut expected = circle_dots(-3.0, &[-3.0, -2.0, -1.0, 1.0, 2.0, 3.0]);
    expected.extend(circle_dots(0.0, &[-4.0, 4.0]));
    expected.extend(circle_dots(3.0, &[-3.0, -2.0, -1.0, 1.0, 2.0, 3.0]));
    assert_eq!(expected.len(), 14);
    assert_exact_circle_output(&p, &r, &expected, 0.0);
}

#[test]
fn annular_secants_beyond_the_hole_keep_the_middle_line_and_dot() {
    let r = circular_region(true);
    for q in [-4.0, 4.0] {
        let continuous = pattern(vec![family([2.0, -1.0 + q], 0.0, 32.0, &[])]);
        assert_exact_circle_output(&continuous, &r, &circle_lines(&[[-3.0, 3.0, q]]), 6.0);
        let dotted = pattern(vec![family([2.0, -1.0 + q], 0.0, 32.0, &[0.0, -1.0])]);
        assert_exact_circle_output(&dotted, &r, &circle_dots(q, &[-2.0, -1.0, 0.0, 1.0, 2.0]), 0.0);
    }
}

fn phased_annulus_lines() -> Vec<Mark> {
    // Exact period 3, phase k/2: clipping neither restarts a cycle nor joins
    // the two pieces of the k=-1, j=0 dash across its tangent contact.
    circle_lines(&[
        [-3.5, -1.5, -3.0],
        [-0.5, 0.0, -3.0],
        [0.0, 1.5, -3.0],
        [2.5, 4.0, -3.0],
        [-5.0, -4.0, 0.0],
        [3.0, 5.0, 0.0],
        [-4.0, -3.5, 3.0],
        [-2.5, -0.5, 3.0],
        [0.5, 2.5, 3.0],
        [3.5, 4.0, 3.0],
    ])
}

#[test]
fn signed_annular_dash_phase_preserves_ten_pieces_and_thirteen_and_a_half_length() {
    let p = pattern(vec![family([2.0, -1.0], 0.5, 3.0, &[2.0, -1.0])]);
    assert_exact_circle_output(&p, &circular_region(true), &phased_annulus_lines(), 13.5);
}

#[test]
fn annular_zero_tokens_keep_thirty_ordered_instances_across_span_windows() {
    let p = pattern(vec![family([2.0, -1.0], 0.5, 3.0, &[0.0, 0.0, -2.0, 0.0])]);
    let mut positions = circle_dots(-3.0, &[-2.5, -0.5, 1.5, 3.5]);
    positions.extend(circle_dots(0.0, &[-4.0, 4.0]));
    positions.extend(circle_dots(3.0, &[-3.5, -1.5, 0.5, 2.5]));
    // At H+2*j: previous cycle's trailing zero, then this cycle's two zeros.
    let expected: Vec<_> = positions.into_iter().flat_map(|mark| [mark; 3]).collect();
    assert_eq!(expected.len(), 30);
    assert_exact_circle_output(&p, &circular_region(true), &expected, 0.0);
}

#[test]
fn accepted_circle_reversals_cyclic_starts_and_translation_preserve_ordered_phase() {
    for reversed in [false, true] {
        for start in 0..2 {
            for shift in [Vec2::ZERO, Vec2::new(11.0, -7.0)] {
                let center = Vec2::new(2.0, -1.0) + shift;
                let mut outer = Loop2::circle(center, 5.0);
                let mut hole = Loop2::circle(center, 3.0);
                if reversed {
                    outer = outer.reversed();
                    hole = hole.reversed();
                }
                outer.segs.rotate_left(start);
                hole.segs.rotate_left(1 - start);
                let r = Region2 { outer, holes: vec![hole] };
                let p = pattern(vec![family([center.x, center.y], 0.5, 3.0, &[2.0, -1.0])]);
                let expected: Vec<_> = phased_annulus_lines()
                    .into_iter()
                    .map(|mark| match mark {
                        Mark::Line { a, b } => Mark::Line { a: a + shift, b: b + shift },
                        Mark::Dot { .. } => panic!("the analytic phased fixture has only lines"),
                    })
                    .collect();
                assert_exact_circle_output(&p, &r, &expected, 13.5);
            }
        }
    }
}

fn near_tangent_root_bracket(numerator: u128) -> [f64; 2] {
    // The analytic squared root is numerator/2^60. Integer square root of
    // numerator*2^40 brackets the root on a 2^-50 lattice, independently of
    // the clipper's floating products, sqrt candidate and contact midpoint.
    assert!(numerator < (17_u128 << 60));
    let square = numerator << 40;
    let lower = square.isqrt();
    let upper = if lower * lower == square { lower } else { lower + 1 };
    assert!(lower * lower <= square && upper * upper >= square);
    assert!(upper <= (1_u128 << 53));
    let unit = 2.0_f64.powi(-50);
    [lower as f64 * unit, upper as f64 * unit]
}

// Reuse the clipping acceptance's 1e-9 mm output comparison for this fixture.
// A geometric contact is the midpoint of a certified Bound, not a correctly
// rounded analytic root. Near q=3-2^-30, the square enclosure near 9 can have
// width 2^-49; propagating it through the small inner root gives about 1e-11
// mm of contact uncertainty. This comparison includes that effect and final
// coordinate rounding; it never chooses topology or relaxes the older tests.
const CIRCLE_CONTACT_COMPARISON_MM: f64 = 1e-9;

fn assert_circle_line_enclosures(marks: &[Mark], y: f64, intervals: &[([f64; 2], [f64; 2])], length: [f64; 2]) {
    assert_eq!(marks.len(), intervals.len());
    let mut total = 0.0;
    for (mark, &(left, right)) in marks.iter().zip(intervals) {
        let Mark::Line { a, b } = *mark else { panic!("continuous circle pattern produced a dot") };
        assert!(a.is_finite() && b.is_finite() && a.x < b.x);
        assert_eq!(a.y, y);
        assert_eq!(b.y, y);
        for (actual, bounds) in [(a.x, left), (b.x, right)] {
            // These reference lattice endpoints and their translation by 2
            // are exactly representable. Only the public contact output gets
            // the documented comparison allowance around the exact-root bracket.
            let low = 2.0 + bounds[0];
            let high = 2.0 + bounds[1];
            assert!(actual >= low - CIRCLE_CONTACT_COMPARISON_MM && actual <= high + CIRCLE_CONTACT_COMPARISON_MM);
        }
        total += b.x - a.x;
    }
    // Each positive horizontal span contributes two endpoint uncertainties.
    let allowance = 2.0 * intervals.len() as f64 * CIRCLE_CONTACT_COMPARISON_MM;
    assert!(total >= length[0] - allowance && total <= length[1] + allowance);
}

#[test]
fn near_inner_tangency_keeps_distinct_gaps_contacts_and_strict_dot_counts() {
    let r = circular_region(true);
    let delta = 2.0_f64.powi(-30);
    let denominator = 1_u128 << 60;
    let six_delta = 6_u128 << 30;
    // Analytic radicands over 2^60 avoid the cancellation of delta^2 in a
    // floating computation of 25-q*q or 9-q*q. Counts are exact: 2/2/1
    // spans and 8/6/7 dots; no success-or-error alternative is accepted.
    for (side, outer_square, inner_square, dot_positions) in [
        (-1, 16 * denominator + six_delta - 1, Some(six_delta - 1), &[-4.0, -3.0, -2.0, -1.0, 1.0, 2.0, 3.0, 4.0][..]),
        (0, 16 * denominator, Some(0), &[-3.0, -2.0, -1.0, 1.0, 2.0, 3.0][..]),
        (1, 16 * denominator - six_delta - 1, None, &[-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0][..]),
    ] {
        let a = near_tangent_root_bracket(outer_square);
        let (intervals, length) = match inner_square {
            Some(square) => {
                let b = near_tangent_root_bracket(square);
                (vec![([-a[1], -a[0]], [-b[1], -b[0]]), (b, a)], [2.0 * (a[0] - b[1]), 2.0 * (a[1] - b[0])])
            }
            None => (vec![([-a[1], -a[0]], a)], [2.0 * a[0], 2.0 * a[1]]),
        };
        for sign in [-1.0, 1.0] {
            let q = sign * (3.0 + f64::from(side) * delta);
            let continuous = pattern(vec![family([2.0, -1.0 + q], 0.0, 32.0, &[])]);
            let marks = circle_output(&continuous, &r, None).unwrap();
            assert_circle_line_enclosures(&marks, -1.0 + q, &intervals, length);
            let Mark::Line { a: first, .. } = marks[0] else { panic!("missing first line") };
            let Mark::Line { b: last, .. } = marks[marks.len() - 1] else { panic!("missing last line") };
            if side < 0 {
                // A>4 and B>0: the comparison allowance cannot conceal an
                // outer endpoint snapped to 4 or a collapsed positive hole gap.
                assert!(first.x < -2.0 && last.x > 6.0);
                let Mark::Line { b: left, .. } = marks[0] else { panic!("missing left span") };
                let Mark::Line { a: right, .. } = marks[1] else { panic!("missing right span") };
                assert!(left.x < 2.0 && right.x > 2.0);
            } else if side == 0 {
                assert_eq!(marks, circle_lines(&[[-4.0, 0.0, q], [0.0, 4.0, q]]));
            } else {
                // A<4 and no hole contact: one span crosses the center.
                assert!(first.x > -2.0 && last.x < 6.0 && first.x < 2.0 && last.x > 2.0);
            }
            assert_eq!(circle_output(&continuous, &r, Some(Limits { marks: intervals.len(), ..Limits::default() })).unwrap(), marks);
            assert!(circle_output(&continuous, &r, Some(Limits { marks: intervals.len() - 1, ..Limits::default() })).is_err());
            let dotted = pattern(vec![family([2.0, -1.0 + q], 0.0, 32.0, &[0.0, -1.0])]);
            assert_exact_circle_output(&dotted, &r, &circle_dots(q, dot_positions), 0.0);
        }
    }
}

#[test]
fn circular_generation_charges_original_pieces_candidates_roots_and_tokens() {
    let continuous = pattern(vec![family([2.0, -1.0], 0.0, 3.0, &[])]);
    for (r, pieces) in [(circular_region(false), 1), (circular_region(true), 3)] {
        let expected = circle_output(&continuous, &r, None).unwrap();
        assert!(!expected.is_empty());
        assert_eq!(circle_output(&continuous, &r, Some(Limits { input_pieces: pieces + 1, ..Limits::default() })).unwrap(), expected);
        for limits in [
            Limits { input_pieces: pieces, ..Limits::default() },
            Limits { candidate_lines: 0, ..Limits::default() },
            // At least k=-3..3 are charged, including exterior candidates.
            // Wider certified bounds may legitimately visit more than seven.
            Limits { candidate_lines: 6, ..Limits::default() },
            Limits { work: 0, ..Limits::default() },
            Limits { events: 0, ..Limits::default() },
        ] {
            assert!(circle_output(&continuous, &r, Some(limits)).is_err());
        }
        let dotted = pattern(vec![family([2.0, -1.0], 0.0, 3.0, &[0.0, -1.0])]);
        assert!(circle_output(&dotted, &r, Some(Limits { dash_steps: 0, ..Limits::default() })).is_err());
    }
}

#[test]
fn empty_circular_output_still_charges_geometric_and_gap_work() {
    let r = circular_region(true);
    let gap = pattern(vec![family([2.0, -1.0], 0.0, 3.0, &[-1.0])]);
    assert_exact_circle_output(&gap, &r, &[], 0.0);
    for limits in [
        Limits { input_pieces: 3, ..Limits::default() },
        Limits { candidate_lines: 0, ..Limits::default() },
        Limits { candidate_lines: 6, ..Limits::default() },
        Limits { work: 0, ..Limits::default() },
        Limits { events: 0, ..Limits::default() },
        Limits { dash_steps: 0, ..Limits::default() },
    ] {
        assert!(circle_output(&gap, &r, Some(limits)).is_err());
    }
    let exterior = pattern(vec![family([2.0, 5.0], 0.0, 32.0, &[0.0, -1.0])]);
    assert_exact_circle_output(&exterior, &r, &[], 0.0);
    assert!(circle_output(&exterior, &r, Some(Limits { candidate_lines: 0, ..Limits::default() })).is_err());
    assert!(circle_output(&exterior, &r, Some(Limits { work: 0, ..Limits::default() })).is_err());
}

// Decimal-spacing fixtures use the exact stored 0.1 = N / 2^55. This integer
// oracle is independent of the generator's dyadic and enclosure implementation.
fn tenth_member(k: i64) -> f64 {
    let numerator = i128::from(k) * 3_602_879_701_896_397;
    numerator as f64 / 36_028_797_018_963_968.0
}

#[test]
fn decimal_spacing_keeps_every_signed_member_in_a_polygon() {
    let r = region(-1.0, -0.95, 1.0, 0.95);
    let before = r.clone();
    let lines: Vec<_> = (-9..=9).map(|k| [-1.0, tenth_member(k), 1.0, tenth_member(k)]).collect();
    assert_eq!(lines.len(), 19);
    for dy in [0.1, -0.1] {
        let p = pattern(vec![family([0.0, 0.0], 0.0, dy, &[])]);
        let saved = p.clone();
        assert_marks(&generate(&p, &r).unwrap(), &lines, &[], 0.0);
        assert_eq!(p, saved);
        assert_eq!(r, before);
    }
}

#[test]
fn decimal_spacing_keeps_dot_phase_and_holes_without_partial_output() {
    let mut r = region(-0.25, -0.35, 2.25, 0.35);
    r.holes.push(rectangle(0.75, -0.15, 1.25, 0.15).reversed());
    let mut dots = Vec::new();
    for k in -3i64..=3 {
        let xs: &[f64] = if k == 0 {
            &[0.0, 2.0]
        } else if k % 2 == 0 {
            &[0.0, 1.0, 2.0]
        } else {
            &[0.5, 1.5]
        };
        dots.extend(xs.iter().map(|&x| [x, tenth_member(k)]));
    }
    assert_eq!(dots.len(), 16);
    for (dx, dy) in [(0.5, 0.1), (-0.5, -0.1)] {
        let p = pattern(vec![family([0.0, 0.0], dx, dy, &[0.0, -1.0])]);
        assert_marks(&generate_with_limits(&p, &r, Limits { marks: 16, ..Limits::default() }).unwrap(), &[], &dots, 0.0);
        assert!(generate_with_limits(&p, &r, Limits { marks: 15, ..Limits::default() }).is_err());
    }
}

#[test]
fn decimal_spacing_clips_disks_and_annuli_with_the_same_offset_enclosure() {
    for hole in [false, true] {
        let r =
            Region2 { outer: Loop2::circle(Vec2::ZERO, 1.125), holes: if hole { vec![Loop2::circle(Vec2::ZERO, 0.25).reversed()] } else { vec![] } };
        let mut lines = Vec::new();
        for k in -11..=11 {
            let y = tenth_member(k);
            let outer = (1.265625 - y * y).sqrt();
            if hole && (-2..=2).contains(&k) {
                let inner = (0.0625 - y * y).sqrt();
                lines.extend([[-outer, y, -inner, y], [inner, y, outer, y]]);
            } else {
                lines.push([-outer, y, outer, y]);
            }
        }
        assert_eq!(lines.len(), if hole { 28 } else { 23 });
        for dy in [0.1, -0.1] {
            let p = pattern(vec![family([0.0, 0.0], 0.0, dy, &[])]);
            assert_marks(&generate(&p, &r).unwrap(), &lines, &[], ROUNDOFF_MM);
        }
    }
}

#[test]
fn decimal_spacing_contact_uncertainty_is_not_replaced_by_a_rounded_line() {
    // 3 * stored(0.1) is strictly between stored(0.3) and its successor.
    // Its enclosure touches the lower edge: guessing from nearest() would
    // silently draw a stroke instead of reporting that unresolved contact.
    let r = region(-1.0, 0.3, 1.0, 0.4);
    let p = pattern(vec![family([0.0, 0.0], 0.0, 0.1, &[])]);
    assert!(generate(&p, &r).is_err());
}

#[test]
fn decimal_spacing_preserves_cumulative_limits_and_gap_only_semantics() {
    let r = region(-1.0, -0.95, 1.0, 0.95);
    let p = pattern(vec![family([0.0, 0.0], 0.0, 0.1, &[])]);
    assert_eq!(generate_with_limits(&p, &r, Limits { marks: 19, ..Limits::default() }).unwrap().len(), 19);
    for limits in [
        Limits { marks: 18, ..Limits::default() },
        Limits { candidate_lines: 18, ..Limits::default() },
        Limits { input_pieces: 3, ..Limits::default() },
        Limits { work: 0, ..Limits::default() },
        Limits { events: 0, ..Limits::default() },
    ] {
        assert!(generate_with_limits(&p, &r, limits).is_err());
    }
    let gap = pattern(vec![family([0.0, 0.0], 0.5, 0.1, &[-1.0])]);
    assert!(generate(&gap, &r).unwrap().is_empty());
    assert!(generate_with_limits(&gap, &r, Limits { dash_steps: 0, ..Limits::default() }).is_err());
}

#[test]
fn decimal_spacing_strokes_keep_their_phase_across_hole_fragments() {
    let mut r = region(-0.25, -0.35, 2.25, 0.35);
    r.holes.push(rectangle(0.75, -0.15, 1.25, 0.15).reversed());
    // Member k starts at k/2 and repeats [1,-1]. The k=0 and k=1
    // strokes meet the hole; clipping must not restart their two-mm cycle.
    let endpoints: &[(i64, &[(f64, f64)])] = &[
        (-3, &[(0.5, 1.5)]),
        (-2, &[(-0.25, 0.0), (1.0, 2.0)]),
        (-1, &[(-0.25, 0.5), (1.5, 2.25)]),
        (0, &[(0.0, 0.75), (2.0, 2.25)]),
        (1, &[(0.5, 0.75), (1.25, 1.5)]),
        (2, &[(-0.25, 0.0), (1.0, 2.0)]),
        (3, &[(-0.25, 0.5), (1.5, 2.25)]),
    ];
    let lines: Vec<_> = endpoints.iter().flat_map(|&(k, spans)| spans.iter().map(move |&(a, b)| [a, tenth_member(k), b, tenth_member(k)])).collect();
    assert_eq!(lines.len(), 13);
    for (dx, dy) in [(0.5, 0.1), (-0.5, -0.1)] {
        let p = pattern(vec![family([0.0, 0.0], dx, dy, &[1.0, -1.0])]);
        assert_marks(&generate(&p, &r).unwrap(), &lines, &[], 0.0);
    }
}

#[test]
fn decimal_spacing_circle_contact_is_not_invented_by_nearest_rounding() {
    // The third exact offset is between adjacent floats. Treating nearest()
    // as the exact offset could guess a miss or tangency at these radii.
    for radius in [0.3, tenth_member(3)] {
        let r = Region2 { outer: Loop2::circle(Vec2::ZERO, radius), holes: vec![] };
        let p = pattern(vec![family([0.0, 0.0], 0.0, 0.1, &[])]);
        assert!(generate(&p, &r).is_err(), "uncertain contact at radius {radius}");
    }
}
