//! Analytic cursor oracle plus feasibility checks on every accepted continuation step.
use solidcraft_geom::Vec2;
use solidcraft_sketch::{ConstraintKind, PointIdx, Sketch, move_line_towards, move_point_towards, solve};

fn linkage() -> (Sketch, PointIdx, PointIdx) {
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(-10.0, 0.0), None).unwrap();
    let x = -10.0 + (1600.0 - 400.0 + 52.0 * 52.0) / 104.0;
    let c = sk.add_point(Vec2::new(x, -(1600.0_f64 - (x + 10.0).powi(2)).sqrt()), None).unwrap();
    let b = sk.add_point(Vec2::new(42.0, 0.0), None).unwrap();
    sk.points[b].fixed = true;
    for (p, q, value) in [(PointIdx::ORIGIN, a, 10.0), (a, c, 40.0), (c, b, 20.0)] {
        sk.add_constraint(ConstraintKind::Distance { p, q, value }, None).unwrap();
    }
    assert!(solve(&mut sk).ok());
    (sk, a, c)
}

#[test]
fn a_material_point_on_a_link_follows_without_relaxing_its_length() {
    for along in [0.2, 0.5, 0.8] {
        for target in [Vec2::new(-10.0, 16.0), Vec2::new(20.0, 0.0), Vec2::new(1000.0, -1000.0)] {
            let (mut sk, a, c) = linkage();
            let line = sk.add_line_pts(PointIdx::ORIGIN, a, None).unwrap();
            let target = target * along;
            assert!(move_line_towards(&mut sk, line, Vec2::new(along, 0.0), target, 1.25, None, |before, after| {
                assert!((after.points[a].pos.len() - 10.0).abs() < 1e-9);
                assert!((after.points[a].pos.dist(after.points[c].pos) - 40.0).abs() < 1e-9);
                assert!((after.points[c].pos.dist(Vec2::new(42.0, 0.0)) - 20.0).abs() < 1e-9);
                assert!((after.points[a].pos * along).dist(target) < (before.points[a].pos * along).dist(target));
                (Vec2::new(42.0, 0.0) - after.points[a].pos).cross(after.points[c].pos - after.points[a].pos) < 0.0
            }));
            let closest = target * (10.0 * along / target.len());
            assert!((sk.points[a].pos * along).dist(closest) < 1e-4);
            let before = sk.clone();
            for invalid in [Vec2::new(f64::NAN, 0.0), Vec2::new(0.5, f64::INFINITY), Vec2::new(1e300, 1e300)] {
                assert!(!move_line_towards(&mut sk, line, invalid, target, 1.25, None, |_, _| true));
                assert_eq!(sk, before);
            }
        }
    }
}

#[test]
fn circle_oracle_all_directions_and_large_targets() {
    for radius in [1.0, 12.8, 28.3, 100.0, 1000.0] {
        for i in 0..24 {
            let (mut sk, a, c) = linkage();
            let angle = std::f64::consts::TAU * i as f64 / 24.0;
            let target = Vec2::new(angle.cos(), angle.sin()) * radius;
            let nearest = target * (10.0 / radius);
            assert!(move_point_towards(&mut sk, a, target, 1.25, None, |before, after| {
                assert!(after.points[a].pos.dist(target) < before.points[a].pos.dist(target));
                assert!((after.points[a].pos.len() - 10.0).abs() < 1e-9);
                assert!((after.points[a].pos.dist(after.points[c].pos) - 40.0).abs() < 1e-9);
                assert!((after.points[c].pos.dist(Vec2::new(42.0, 0.0)) - 20.0).abs() < 1e-9);
                (Vec2::new(42.0, 0.0) - after.points[a].pos).cross(after.points[c].pos - after.points[a].pos) < 0.0
            }));
            assert!(sk.points[a].pos.dist(nearest) < 1e-4, "radius {radius}, angle {angle}, actual {:?}, nearest {nearest:?}", sk.points[a].pos);
        }
    }
}

#[test]
fn fixed_axis_and_rejected_candidates_keep_design_constraints() {
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
    let l = sk.add_line_pts(PointIdx::ORIGIN, a, None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    assert!(move_point_towards(&mut sk, a, Vec2::new(20.0, 10.0), 2.0, None, |_, _| true));
    assert!(sk.points[a].pos.dist(Vec2::new(20.0, 0.0)) < 1e-5, "{:?}", sk.points[a].pos);
    let before = sk.clone();
    assert!(move_point_towards(&mut sk, a, Vec2::new(-20.0, 10.0), 2.0, None, |_, _| false));
    assert_eq!(sk, before);
    assert!(!move_point_towards(&mut sk, PointIdx(usize::MAX), Vec2::ZERO, 1.0, None, |_, _| true));
    assert!(!move_point_towards(&mut sk, a, Vec2::new(f64::NAN, 0.0), 1.0, None, |_, _| true));
    assert_eq!(sk, before);
    sk.points[a].pos.x = f64::NAN;
    assert!(!move_point_towards(&mut sk, a, Vec2::ZERO, 1.0, None, |_, _| true));
    assert!(sk.points[a].pos.x.is_nan());
}

#[test]
fn reversing_across_a_nearly_stationary_extremum_keeps_following() {
    let (mut sk, a, _) = linkage();
    for target in [Vec2::new(-10.0, 16.0), Vec2::new(-10.0, -16.0), Vec2::new(0.0, 25.0), Vec2::new(0.0, -25.0)] {
        assert!(move_point_towards(&mut sk, a, target, 1.25, None, |_, _| true));
        assert!(sk.points[a].pos.dist(target * (10.0 / target.len())) < 1e-4, "target {target:?}, actual {:?}", sk.points[a].pos);
    }
}

#[test]
fn accepted_constraint_roundoff_is_refined_before_following_the_cursor() {
    let (mut sk, a, _) = linkage();
    sk.points[a].pos.x -= 1e-8;
    let target = Vec2::new(-10.0, 8.0);
    assert!(move_point_towards(&mut sk, a, target, 1.25, None, |_, _| true));
    assert!(sk.points[a].pos.dist(target * (10.0 / target.len())) < 1e-4);
    assert!((sk.points[a].pos.len() - 10.0).abs() < 1e-9);
}

#[test]
fn a_cursor_at_the_pivot_does_not_choose_a_pose_from_roundoff() {
    let (mut sk, a, _) = linkage();
    let before = sk.clone();
    assert!(move_point_towards(&mut sk, a, Vec2::ZERO, 1.25, None, |_, _| true));
    assert!(sk.points[a].pos.dist(before.points[a].pos) < 1e-5, "{:?}", sk.points[a].pos);
}

#[test]
fn cursor_motion_refreshes_driven_dimensions_without_a_followup_solve() {
    let (mut sk, a, _) = linkage();
    sk.add_constraint(ConstraintKind::DistanceX { p: PointIdx::ORIGIN, q: a, value: 10.0 }, None).unwrap();
    sk.constraints.last_mut().unwrap().driven = true;
    assert!(move_point_towards(&mut sk, a, Vec2::new(-10.0, 8.0), 1.25, None, |_, _| true));
    let value = sk.constraints.last().unwrap().kind.value().unwrap();
    assert!((value - sk.points[a].pos.x.abs()).abs() < 1e-6, "reference {value} does not measure {:?}", sk.points[a].pos);
}

#[test]
fn the_other_moving_joint_reaches_its_rocker_arc_and_motion_limits() {
    let base = Vec2::new(42.0, 0.0);
    let lo = -(-1264.0_f64 / 1680.0).acos(); // |OC| >= 40 - 10
    let hi = -(0.2_f64).acos(); // |OC| <= 40 + 10
    for angle in [-2.6_f64, -2.1, -1.5, -1.1] {
        let (mut sk, a, c) = linkage();
        let target = base + Vec2::new(angle.cos(), angle.sin()) * 60.0;
        let nearest = base + Vec2::new(angle.clamp(lo, hi).cos(), angle.clamp(lo, hi).sin()) * 20.0;
        assert!(move_point_towards(&mut sk, c, target, 1.25, None, |_, after| {
            (base - after.points[a].pos).cross(after.points[c].pos - after.points[a].pos) < 0.0
        }));
        assert!(sk.points[c].pos.dist(nearest) < 1e-4, "angle {angle}, actual {:?}, nearest {nearest:?}", sk.points[c].pos);
        assert!((sk.points[a].pos.len() - 10.0).abs() < 1e-9);
        assert!((sk.points[a].pos.dist(sk.points[c].pos) - 40.0).abs() < 1e-9);
        assert!((sk.points[c].pos.dist(base) - 20.0).abs() < 1e-9);
    }
}

#[test]
fn zero_cursor_budget_does_not_start_optimization() {
    let (mut sk, a, _) = linkage();
    let before = sk.clone();
    let mut budget = solidcraft_sketch::Budget::new(0);
    assert!(!move_point_towards(&mut sk, a, Vec2::new(-10.0, 16.0), 1.25, Some(&mut budget), |_, _| {
        panic!("an exhausted budget must not evaluate candidates")
    }));
    assert_eq!(sk, before);
    assert_eq!(budget.spent(), 0);
    assert!(budget.exhausted());
}

#[cfg(feature = "solver-profiling")]
#[test]
fn profiling_preserves_budgeted_cursor_motion_and_records_budget_stops() {
    use solidcraft_sketch::profiling::{StopReason, capture};

    let target = Vec2::new(-10.0, 16.0);
    let mut saw_budget_stop = false;
    for work in [0, 128, 256, 512, 1024, 2048, 4096, 8192, 16384] {
        let (mut plain, a, _) = linkage();
        let mut measured = plain.clone();
        let mut plain_budget = solidcraft_sketch::Budget::new(work);
        let mut measured_budget = solidcraft_sketch::Budget::new(work);
        let expected = move_point_towards(&mut plain, a, target, 1.25, Some(&mut plain_budget), |_, _| true);
        let (actual, profile) = capture(|| move_point_towards(&mut measured, a, target, 1.25, Some(&mut measured_budget), |_, _| true));
        assert_eq!(actual, expected);
        assert_eq!(measured, plain);
        assert_eq!(measured_budget.spent(), plain_budget.spent());
        assert_eq!(measured_budget.exhausted(), plain_budget.exhausted());
        assert!(profile.runs.iter().all(|run| run.stop_reason.is_some() && run.final_max_residual.is_some()));
        saw_budget_stop |= profile.runs.iter().any(|run| run.stop_reason == Some(StopReason::WorkBudget));
        if work == 0 {
            assert_eq!(profile.counters.lm_runs, 0);
        }
    }
    assert!(saw_budget_stop, "exercise LM returning at its shared work limit");
}

#[test]
fn a_cursor_budget_stop_keeps_the_best_corrected_pose_and_reference_dimensions() {
    let target = Vec2::new(-10.0, 16.0);
    let nearest = target * (10.0 / target.len());
    let mut stopped_after_progress = false;
    for work in [128, 256, 512, 1024, 2048, 4096, 8192, 16384] {
        let (mut sk, a, c) = linkage();
        sk.add_constraint(ConstraintKind::DistanceX { p: PointIdx::ORIGIN, q: a, value: 10.0 }, None).unwrap();
        sk.constraints.last_mut().unwrap().driven = true;
        let before = sk.clone();
        let mut budget = solidcraft_sketch::Budget::new(work);
        // The validation callback is Fn: Cell records whether a corrected step was accepted.
        let calls = std::cell::Cell::new(0);
        let moved = move_point_towards(&mut sk, a, target, 1.25, Some(&mut budget), |_, after| {
            calls.set(calls.get() + 1);
            assert!((after.points[a].pos.len() - 10.0).abs() < 1e-9);
            assert!((after.points[a].pos.dist(after.points[c].pos) - 40.0).abs() < 1e-9);
            assert!((after.points[c].pos.dist(Vec2::new(42.0, 0.0)) - 20.0).abs() < 1e-9);
            true
        });
        assert!(budget.spent() <= work, "{budget:?}");
        if !moved {
            assert_eq!(sk, before);
            continue;
        }
        assert!((sk.points[a].pos.len() - 10.0).abs() < 1e-9);
        assert!((sk.constraints.last().unwrap().kind.value().unwrap() - sk.points[a].pos.x.abs()).abs() < 1e-6);
        if calls.get() > 0 && budget.exhausted() && sk.points[a].pos.dist(nearest) > 1e-4 {
            assert!(sk.points[a].pos.dist(target) < before.points[a].pos.dist(target));
            stopped_after_progress = true;
        }
    }
    assert!(stopped_after_progress, "exercise exhaustion after a feasible cursor improvement");
}
