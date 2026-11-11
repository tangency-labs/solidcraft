//! Tests of machine kinematics, the solving of toolpaths into machine moves, and indexed (3+1,
//! 3+2) setups.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_record::{Loop2, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::foundation_tests::*;
use crate::indexed::{frame_for_axis, frame_for_index, index, index_for_axis};
use crate::kinematics::*;
use crate::*;

fn deg(d: f64) -> f64 {
    d.to_radians()
}

/// A small deterministic random sequence in `0..1`.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

/// The tool axis `θ` round the X axis, radial outwards: `+Z` at zero, `+Y` at 90° (what `A = θ`
/// brings up to the tool), as 4th-axis toolpaths give it.
fn radial(theta: f64) -> Vec3 {
    Vec3::new(0.0, theta.sin(), theta.cos())
}

/// Every preset, some with their centres, pivots and work origin moved, and a reversed one.
fn machines() -> Vec<Machine> {
    vec![
        Machine::three_axis(),
        Machine::four_axis_a(),
        Machine::four_axis_a_at(Vec3::new(0.0, 3.0, -40.0)).with_work_origin(Vec3::new(5.0, -2.0, 1.0)),
        Machine::four_axis_a_with(Vec3::ZERO, true, None),
        Machine::table_table_ac(),
        Machine::table_table_ac_at(Vec3::new(0.0, 0.0, -50.0), Vec3::new(10.0, 5.0, -50.0)).with_work_origin(Vec3::new(20.0, -10.0, 30.0)),
        Machine::table_table_bc(),
        Machine::table_table_bc_at(Vec3::new(4.0, 0.0, -30.0), Vec3::new(-6.0, 2.0, 0.0)).with_work_origin(Vec3::new(0.0, 15.0, 10.0)),
        Machine::head_table_bc(),
        Machine::head_table_bc_at(150.0, Vec3::new(30.0, -20.0, 0.0)).with_work_origin(Vec3::new(1.0, 2.0, 3.0)),
        Machine::head_head_ca(),
        Machine::head_head_ca_at(220.0).with_work_origin(Vec3::new(-50.0, 0.0, 12.0)),
    ]
}

/// Random joints within the machine's limits (kept a little off them).
fn random_joints(m: &Machine, rng: &mut Lcg) -> Joints {
    let rotary = m
        .rotaries
        .iter()
        .map(|r| match r.limits {
            Some((lo, hi)) => rng.range(lo + 0.05, hi - 0.05),
            None => rng.range(-3.0 * PI, 3.0 * PI),
        })
        .collect();
    Joints { linear: Vec3::new(rng.range(-200.0, 200.0), rng.range(-200.0, 200.0), rng.range(-200.0, 200.0)), rotary }
}

#[test]
fn forward_of_inverse_is_the_pose_on_every_preset() {
    let mut rng = Lcg(7);
    for m in machines() {
        m.validate().unwrap();
        for _ in 0..200 {
            let pose = forward(&m, &random_joints(&m, &mut rng));
            for previous in [None, Some(random_joints(&m, &mut rng))] {
                let j = inverse(&m, &pose, previous.as_ref()).unwrap();
                let back = forward(&m, &j);
                close3(back.tip, pose.tip, 1e-9);
                close3(back.axis, pose.axis, 1e-12);
            }
        }
    }
}

#[test]
fn every_solution_points_the_tool() {
    let mut rng = Lcg(11);
    for m in machines() {
        for _ in 0..100 {
            let pose = forward(&m, &random_joints(&m, &mut rng));
            let sols = solutions(&m, pose.axis).unwrap();
            assert!(!sols.is_empty());
            if m.rotaries.len() == 2 {
                // Away from the singular directions both meeting points of the cones are real.
                assert_eq!(sols.len(), 2, "{}", m.name);
            }
            for s in &sols {
                close3(tool_axis(&m, s), pose.axis, 1e-12);
                assert!(s.iter().all(|a| a.abs() <= PI + 1e-12));
            }
        }
    }
}

#[test]
fn four_axis_turns_the_pose_about_x() {
    // The work origin on the centreline: the written X Y Z are the tip turned by A about X.
    let m = Machine::four_axis_a();
    let tip = Vec3::new(10.0, 20.0, 5.0);
    let j = inverse(&m, &Pose::new(tip, radial(deg(30.0))), None).unwrap();
    close(j.rotary[0], deg(30.0), 1e-12);
    let (s, c) = deg(30.0).sin_cos();
    close3(j.linear, Vec3::new(10.0, 20.0 * c - 5.0 * s, 20.0 * s + 5.0 * c), 1e-12);
    close3(j.linear, rotate(tip, Vec3::X, deg(30.0)), 1e-12);
    // A point on the tool's radial line goes straight above the axis.
    let on_line = Pose::new(Vec3::new(7.0, 0.0, 0.0) + radial(deg(-75.0)) * 12.0, radial(deg(-75.0)));
    let j = inverse(&m, &on_line, None).unwrap();
    close(j.rotary[0], deg(-75.0), 1e-12);
    close3(j.linear, Vec3::new(7.0, 0.0, 12.0), 1e-12);

    // A controller whose positive A turns the other way: the same place, the angle negated.
    let rev = Machine::four_axis_a_with(Vec3::ZERO, true, None);
    let jr = inverse(&rev, &Pose::new(tip, radial(deg(30.0))), None).unwrap();
    close(jr.rotary[0], deg(-30.0), 1e-12);
    close3(jr.linear, rotate(tip, Vec3::X, deg(30.0)), 1e-12);

    // The centreline off the work origin: the work turns about its own line.
    let center = Vec3::new(0.0, 4.0, -40.0);
    let off = Machine::four_axis_a_at(center);
    let jo = inverse(&off, &Pose::new(tip, radial(deg(30.0))), None).unwrap();
    close3(jo.linear, rotate_about(tip, center, Vec3::X, deg(30.0)), 1e-12);

    // With the tool vertical the 4th axis stays at zero (or where it was, plus whole turns).
    let up = Pose::vertical(tip);
    assert_eq!(inverse(&m, &up, None).unwrap().rotary, vec![0.0]);
    let after = inverse(&m, &up, Some(&Joints { linear: Vec3::ZERO, rotary: vec![deg(725.0)] })).unwrap();
    close(after.rotary[0], deg(720.0), 1e-12);
}

#[test]
fn a_continuous_rotary_adds_up_past_a_turn() {
    // A wrapped helix: three turns round the X axis at radius 20 while the tip moves 30 along X.
    let m = Machine::four_axis_a();
    let n = 360;
    let pose = |k: usize| {
        let th = 3.0 * TAU * k as f64 / n as f64;
        Pose::new(Vec3::new(30.0 * k as f64 / n as f64, 0.0, 0.0) + radial(th) * 20.0, radial(th))
    };
    let mut moves = vec![Move5::Rapid { to: pose(0) }];
    moves.extend((1..=n).map(|k| Move5::Feed { to: pose(k), feed: 300.0 }));
    let out = solve(&m, &Toolpath5 { moves }, &SolveOptions::default()).unwrap();
    let last = out.last().and_then(JointMove::joints).unwrap();
    close(last.rotary[0], 3.0 * TAU, 1e-9);
    close3(last.linear, Vec3::new(30.0, 0.0, 20.0), 1e-9);
    // Every step turns A the same way.
    let angles: Vec<f64> = out.iter().filter_map(JointMove::joints).map(|j| j.rotary[0]).collect();
    assert!(angles.windows(2).all(|w| w[1] >= w[0] - 1e-12));

    // Limits stop it: a table that turns at most ±90° cannot follow.
    let limited = Machine::four_axis_a_with(Vec3::ZERO, false, Some((deg(-90.0), deg(90.0))));
    let err = solve(
        &limited,
        &Toolpath5 { moves: vec![Move5::Rapid { to: pose(0) }, Move5::Rapid { to: Pose::new(Vec3::ZERO, radial(deg(120.0))) }] },
        &SolveOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(&err, CamError::Unsafe(s) if s.contains("move 1") && s.contains('A')), "{err:?}");
}

#[test]
fn unreachable_directions_are_refused() {
    let tilted_about_y = Vec3::new(deg(10.0).sin(), 0.0, deg(10.0).cos());
    let pose = Pose::new(Vec3::ZERO, tilted_about_y);
    assert!(matches!(inverse(&Machine::four_axis_a(), &pose, None), Err(CamError::Invalid(_))));
    assert!(matches!(inverse(&Machine::three_axis(), &pose, None), Err(CamError::Invalid(_))));
    assert!(inverse(&Machine::three_axis(), &Pose::vertical(Vec3::new(1.0, 2.0, 3.0)), None).is_ok());
    // Beyond the AC trunnion's ±120°: pointing the tool down from below.
    let below = Pose::new(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0));
    assert!(matches!(inverse(&Machine::table_table_ac(), &below, None), Err(CamError::Unsafe(_))));
    // The head of a head-table machine reaches only ±120° of tilt as well.
    let mut low = Machine::head_table_bc();
    low.rotaries[0].limits = Some((deg(-30.0), deg(30.0)));
    assert!(inverse(&low, &Pose::new(Vec3::ZERO, radial(deg(60.0))), None).is_err());
}

#[test]
fn ac_table_by_hand() {
    // Turning the work right-handed about +X by A and about +Z by C: the tool axis
    // (0, −sin 30°, cos 30°) comes up to +Z with A = −30°, C = 0 (or A = +30°, C = 180°).
    let m = Machine::table_table_ac();
    let axis = Vec3::new(0.0, -deg(30.0).sin(), deg(30.0).cos());
    let mut sols = solutions(&m, axis).unwrap();
    sols.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(sols.len(), 2);
    close(sols[0][0], deg(-30.0), 1e-12);
    close(sols[0][1], 0.0, 1e-12);
    close(sols[1][0], deg(30.0), 1e-12);
    close(sols[1][1].abs(), PI, 1e-12);
    // With no previous position the one nearest zero.
    let tip = Vec3::new(10.0, 20.0, 5.0);
    let j = inverse(&m, &Pose::new(tip, axis), None).unwrap();
    close(j.rotary[0], deg(-30.0), 1e-12);
    close(j.rotary[1], 0.0, 1e-12);
    // The tip by hand: y' = 20 cos 30° + 5 sin 30°, z' = −20 sin 30° + 5 cos 30°.
    close3(j.linear, Vec3::new(10.0, 20.0 * 0.75f64.sqrt() + 2.5, -10.0 + 5.0 * 0.75f64.sqrt()), 1e-12);
    // Near the other solution, that one.
    let near = Joints { linear: Vec3::ZERO, rotary: vec![deg(25.0), deg(170.0)] };
    let j = inverse(&m, &Pose::new(tip, axis), Some(&near)).unwrap();
    close(j.rotary[0], deg(30.0), 1e-12);
    close(j.rotary[1], PI, 1e-12);
}

#[test]
fn limits_pick_the_solution_in_range() {
    let mut m = Machine::table_table_ac();
    m.rotaries[0].limits = Some((0.0, deg(120.0)));
    let axis = Vec3::new(0.0, -deg(30.0).sin(), deg(30.0).cos());
    // The previous position favours A = −30°, which is out of range.
    let prev = Joints { linear: Vec3::ZERO, rotary: vec![deg(-29.0), 0.0] };
    let j = inverse(&m, &Pose::new(Vec3::ZERO, axis), Some(&prev)).unwrap();
    close(j.rotary[0], deg(30.0), 1e-12);
    close(j.rotary[1], PI, 1e-12);
    close3(forward(&m, &j).axis, axis, 1e-12);
}

#[test]
fn continuity_unwinds_c_past_half_a_turn() {
    let m = Machine::table_table_ac();
    let tilt = deg(20.0);
    // The axis that C = 190° (written −170° in −π..π) brings up.
    let at = |c: f64| tool_axis(&m, &[-tilt, c]);
    let prev = Joints { linear: Vec3::ZERO, rotary: vec![-tilt, deg(175.0)] };
    let j = inverse(&m, &Pose::new(Vec3::ZERO, at(deg(190.0))), Some(&prev)).unwrap();
    close(j.rotary[0], -tilt, 1e-12);
    close(j.rotary[1], deg(190.0), 1e-12);
    // Several turns on, it keeps counting.
    let prev = Joints { linear: Vec3::ZERO, rotary: vec![-tilt, deg(725.0)] };
    let j = inverse(&m, &Pose::new(Vec3::ZERO, at(deg(10.0))), Some(&prev)).unwrap();
    close(j.rotary[1], deg(730.0), 1e-12);
}

#[test]
fn a_vertical_tool_keeps_the_previous_c() {
    let m = Machine::table_table_ac();
    let up = Pose::vertical(Vec3::new(5.0, 6.0, 7.0));
    let prev = Joints { linear: Vec3::ZERO, rotary: vec![0.3, 1.234] };
    let j = inverse(&m, &up, Some(&prev)).unwrap();
    close(j.rotary[0], 0.0, 1e-12);
    close(j.rotary[1], 1.234, 1e-12);
    let j = inverse(&m, &up, None).unwrap();
    assert_eq!(j.rotary, vec![0.0, 0.0]);
    // The fork head with the tool vertical: C stays as well.
    let head = Machine::head_head_ca();
    let j = inverse(&head, &up, Some(&Joints { linear: Vec3::ZERO, rotary: vec![-2.0, 0.4] })).unwrap();
    close(j.rotary[0], -2.0, 1e-12);
    close(j.rotary[1], 0.0, 1e-12);
    close3(forward(&head, &j).tip, up.tip, 1e-12);
}

#[test]
fn head_pivots_carry_the_tip() {
    // B head tilted 90°: the tip swings round the pivot, the pivot stays put.
    let pivot = 150.0;
    let m = Machine::head_table_bc_at(pivot, Vec3::ZERO);
    let j = inverse(&m, &Pose::new(Vec3::ZERO, Vec3::X), None).unwrap();
    close(j.rotary[0], FRAC_PI_2, 1e-12);
    // The tip at the work origin: the control point is where the pivot is less the tool's
    // pivot length along the tool, i.e. X = −(tip offset of the turned head).
    close3(j.linear, Vec3::new(pivot, 0.0, -pivot), 1e-9);
    close3(j.linear + Vec3::new(0.0, 0.0, pivot), Vec3::X * pivot, 1e-9);
}

/// A table-table move turning C by 90° at A = −30°, the tip 60 mm off the rotary centre.
fn big_turn(m: &Machine) -> Toolpath5 {
    let a0 = tool_axis(m, &[deg(-30.0), 0.0]);
    let a1 = tool_axis(m, &[deg(-30.0), deg(90.0)]);
    Toolpath5 {
        moves: vec![
            Move5::Rapid { to: Pose::new(Vec3::new(60.0, 0.0, 10.0), a0) },
            Move5::Feed { to: Pose::new(Vec3::new(60.0, 10.0, 10.0), a1), feed: 500.0 },
        ],
    }
}

#[test]
fn cuts_are_broken_into_pieces_within_tolerance() {
    let m = Machine::table_table_ac_at(Vec3::ZERO, Vec3::ZERO).with_work_origin(Vec3::new(0.0, 0.0, 20.0));
    let path = big_turn(&m);
    let opts = SolveOptions { tolerance: 0.01, angle_tolerance: deg(0.1), ..SolveOptions::default() };
    let out = solve(&m, &path, &opts).unwrap();
    let pieces: Vec<&JointMove> = out.iter().filter(|j| matches!(j, JointMove::Feed { .. })).collect();
    assert!(pieces.len() > 4, "{} pieces", pieces.len());
    let (Move5::Rapid { to: from }, Move5::Feed { to, .. }) = (path.moves[0], path.moves[1]) else { panic!() };
    let mut prev = out[0].joints().unwrap().clone();
    let mut minutes = 0.0;
    for p in pieces {
        let JointMove::Feed { to: j, minutes: t, .. } = p else { panic!() };
        let mid = forward(&m, &prev.lerp(j, 0.5));
        // Within tolerance of the planned line and turn.
        assert!(mid.tip.dist_to_segment(from.tip, to.tip) <= 0.01 + 1e-12);
        assert!(arc_distance(mid.axis, from.axis, to.axis) <= deg(0.1) + 1e-12);
        minutes += t;
        prev = j.clone();
    }
    // The pieces' times add up to the planned time, and the last piece ends where planned.
    close(minutes, 10.0 / 500.0, 1e-12);
    close3(forward(&m, &prev).tip, to.tip, 1e-9);
    // With tool centre point control the controller keeps the tip on its line: one piece.
    let tcp = solve(&m, &path, &SolveOptions { tcp: true, ..opts }).unwrap();
    assert_eq!(tcp.iter().filter(|j| matches!(j, JointMove::Feed { .. })).count(), 1);
}

#[test]
fn four_axis_cuts_follow_the_chord() {
    // A cut turning A by 20° with the tip on the chord at radius 25: the joint motion bulges
    // out, so it is cut into pieces.
    let m = Machine::four_axis_a();
    let p0 = Pose::new(radial(0.0) * 25.0, radial(0.0));
    let p1 = Pose::new(Vec3::new(5.0, 0.0, 0.0) + radial(deg(20.0)) * 25.0, radial(deg(20.0)));
    let path = Toolpath5 { moves: vec![Move5::Rapid { to: p0 }, Move5::Feed { to: p1, feed: 400.0 }] };
    let out = solve(&m, &path, &SolveOptions::default()).unwrap();
    assert!(out.len() > 3);
    let mut prev = out[0].joints().unwrap().clone();
    for jm in &out[1..] {
        let j = jm.joints().unwrap();
        assert!(forward(&m, &prev.lerp(j, 0.5)).tip.dist_to_segment(p0.tip, p1.tip) <= 0.005 + 1e-12);
        prev = j.clone();
    }
    close(prev.rotary[0], deg(20.0), 1e-12);
}

#[test]
fn a_flip_near_the_pole_is_refused() {
    let m = Machine::table_table_ac();
    let path = |eps: f64| {
        let a0 = Vec3::new(eps.sin(), -deg(3.0).sin(), 1.0).normalized().unwrap();
        let a1 = Vec3::new(eps.sin(), deg(3.0).sin(), 1.0).normalized().unwrap();
        Toolpath5 {
            moves: vec![
                Move5::Rapid { to: Pose::new(Vec3::new(0.0, -1.0, 0.0), a0) },
                Move5::Feed { to: Pose::new(Vec3::new(0.0, 1.0, 0.0), a1), feed: 300.0 },
            ],
        }
    };
    // Straight through vertical the trunnion just tips from one side to the other.
    let through = solve(&m, &path(0.0), &SolveOptions::default()).unwrap();
    let last = through.last().and_then(JointMove::joints).unwrap();
    close(last.rotary[1], 0.0, 1e-12);
    // Passing a third of a degree beside it, C would swing half a turn on the way by.
    let err = solve(&m, &path(deg(0.3)), &SolveOptions::default()).unwrap_err();
    assert!(matches!(&err, CamError::Unsafe(s) if s.contains("move 1") && s.contains("flip")), "{err:?}");
}

#[test]
fn travel_limits_name_the_move_and_axis() {
    let mut m = Machine::four_axis_a();
    m.linear_limits = Some((Vec3::new(-100.0, -100.0, -50.0), Vec3::new(100.0, 100.0, 50.0)));
    let path = Toolpath5 {
        moves: vec![
            Move5::Rapid { to: Pose::vertical(Vec3::new(0.0, 0.0, 10.0)) },
            Move5::Feed { to: Pose::vertical(Vec3::new(0.0, 0.0, 60.0)), feed: 100.0 },
        ],
    };
    let err = solve(&m, &path, &SolveOptions::default()).unwrap_err();
    assert!(matches!(&err, CamError::Unsafe(s) if s.contains("move 1") && s.contains('Z')), "{err:?}");
}

#[test]
fn hostile_machines_and_poses_are_refused() {
    let mut bad = Machine::table_table_ac();
    bad.rotaries[0].direction = Vec3::new(f64::NAN, 0.0, 0.0);
    assert!(bad.validate().is_err());
    let mut three = Machine::table_table_ac();
    three.rotaries.push(three.rotaries[0]);
    assert!(three.validate().is_err());
    let mut parallel = Machine::table_table_ac();
    parallel.rotaries[1].direction = Vec3::X * 2.0;
    assert!(matches!(parallel.validate(), Err(CamError::Unsupported(_))));
    let mut same = Machine::table_table_ac();
    same.rotaries[1].letter = AxisLetter::A;
    assert!(same.validate().is_err());
    let mut limits = Machine::four_axis_a();
    limits.rotaries[0].limits = Some((1.0, -1.0));
    assert!(limits.validate().is_err());
    limits.rotaries[0].limits = Some((f64::NEG_INFINITY, 1.0));
    assert!(limits.validate().is_err());
    let mut origin = Machine::four_axis_a();
    origin.work_origin = Vec3::new(0.0, f64::INFINITY, 0.0);
    assert!(origin.validate().is_err());

    let m = Machine::table_table_ac();
    assert!(inverse(&m, &Pose::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Z), None).is_err());
    assert!(inverse(&m, &Pose::new(Vec3::ZERO, Vec3::ZERO), None).is_err());
    assert!(inverse(&m, &Pose::vertical(Vec3::ZERO), Some(&Joints { linear: Vec3::ZERO, rotary: vec![0.0] })).is_err());
    assert!(inverse(&m, &Pose::vertical(Vec3::ZERO), Some(&Joints { linear: Vec3::ZERO, rotary: vec![0.0, f64::NAN] })).is_err());
    for opts in [
        SolveOptions { tolerance: f64::NAN, ..SolveOptions::default() },
        SolveOptions { angle_tolerance: 0.0, ..SolveOptions::default() },
        SolveOptions { flip_threshold: -1.0, ..SolveOptions::default() },
    ] {
        assert!(solve(&m, &big_turn(&m), &opts).is_err());
    }
    // A cut first, a zero feed, a bad pose.
    let cut_first = Toolpath5 { moves: vec![Move5::Feed { to: Pose::vertical(Vec3::ZERO), feed: 100.0 }] };
    assert!(solve(&m, &cut_first, &SolveOptions::default()).is_err());
    let zero_feed =
        Toolpath5 { moves: vec![Move5::Rapid { to: Pose::vertical(Vec3::ZERO) }, Move5::Feed { to: Pose::vertical(Vec3::X), feed: 0.0 }] };
    assert!(solve(&m, &zero_feed, &SolveOptions::default()).is_err());
    // A tiny tolerance on a big turn needs too many pieces, which is refused rather than run on.
    let tight = SolveOptions { tolerance: 1e-15, angle_tolerance: 1e-15, ..SolveOptions::default() };
    assert!(matches!(solve(&m, &big_turn(&m), &tight), Err(CamError::TooLarge(_))));
    // Forward kinematics never panics on short joint lists.
    let p = forward(&m, &Joints { linear: Vec3::ZERO, rotary: vec![] });
    assert!(p.is_valid());
}

#[test]
fn machines_round_trip_through_json() {
    for m in machines() {
        let text = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Machine>(&text).unwrap(), m);
    }
    let rev = Machine::four_axis_a_with(Vec3::ZERO, true, Some((-1.0, 1.0)));
    let text = serde_json::to_string(&rev).unwrap();
    assert!(text.contains("reversed"));
    assert_eq!(serde_json::from_str::<Machine>(&text).unwrap(), rev);
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
    let v = |x: f64, y: f64| Vec2::new(x, y);
    Region2 { outer: Loop2::polygon(&[v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)]), holes: vec![] }
}

fn contour_path() -> Toolpath {
    let tool = Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0);
    let params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let cut =
        Cut { heights: Heights { clearance: 10.0, retract: 2.0, top: 0.0, bottom: -2.0 }, feeds: Feeds { cut: 600.0, plunge: 200.0 }, stepdown: 1.0 };
    contour(&[rect(0.0, 0.0, 20.0, 10.0)], &tool, &params, &cut).unwrap()
}

#[test]
fn indexed_contour_in_a_tilted_frame() {
    // A contour computed in a frame tilted 45° about X: on an AC table, A = −45°, C = 0.
    let m = Machine::table_table_ac();
    let frame = tilted(Vec3::new(5.0, -3.0, 8.0), deg(45.0), 0.0);
    let path3 = contour_path();
    assert!(path3.moves.iter().any(|mv| matches!(mv, Move::Arc { .. })));
    let ix = index(&path3, &frame, &m, None).unwrap();
    close(ix.rotary[0], deg(-45.0), 1e-12);
    close(ix.rotary[1], 0.0, 1e-12);
    close3(tool_axis(&m, &ix.rotary), frame.z, 1e-12);
    assert!(ix.path.poses().count() >= path3.moves.len());
    for p in ix.path.poses() {
        close3(p.axis, frame.z, 1e-15);
        // And the machine holds those angles all the way.
        let j = inverse(&m, &p, Some(&Joints { linear: Vec3::ZERO, rotary: ix.rotary.clone() })).unwrap();
        close(j.rotary_change(&Joints { linear: Vec3::ZERO, rotary: ix.rotary.clone() }), 0.0, 1e-12);
    }
    // Out of the machine's reach: a 4-axis table cannot tilt the tool about Y.
    assert!(index(&path3, &tilted(Vec3::ZERO, 0.0, deg(30.0)), &Machine::four_axis_a(), None).is_err());
    // Out of its travel.
    let mut small = m.clone();
    small.linear_limits = Some((Vec3::new(-5.0, -5.0, -5.0), Vec3::new(5.0, 5.0, 5.0)));
    assert!(matches!(index(&path3, &frame, &small, None), Err(CamError::Unsafe(_))));
}

#[test]
fn work_planes_line_up_with_the_machine() {
    // The frame for a tool axis has the machine's X and Y as they meet the turned work.
    for m in [Machine::table_table_ac(), Machine::table_table_bc(), Machine::head_table_bc(), Machine::head_head_ca(), Machine::four_axis_a()] {
        let axis = if m.rotaries.len() == 1 { radial(deg(40.0)) } else { Vec3::new(0.3, -0.5, 0.8).normalized().unwrap() };
        let frame = frame_for_axis(&m, axis, Vec3::new(1.0, 2.0, 3.0)).unwrap();
        close3(frame.z, axis, 1e-12);
        let rotary = index_for_axis(&m, axis).unwrap();
        // Moving the tip along the frame's X moves the machine along a fixed direction: its
        // own X (turned with the head on a head machine).
        let at = |p: Vec3| inverse(&m, &Pose::new(frame.to_world(p), axis), None).unwrap();
        let step = at(Vec3::new(10.0, 0.0, 0.0)).linear - at(Vec3::ZERO).linear;
        let along = head_dir(&m, &rotary, Vec3::X) * 10.0;
        close3(step, along, 1e-9);
    }
    // On the 4th axis the index frame is the work turned back by A: the machine position is the
    // frame's coordinates.
    let m = Machine::four_axis_a_at(Vec3::new(0.0, 0.0, -10.0));
    let f = frame_for_index(&m, &[deg(90.0)]).unwrap();
    let p = Vec3::new(3.0, 4.0, 5.0);
    let j = inverse(&m, &Pose::new(f.to_world(p), f.z), None).unwrap();
    close3(j.linear, p, 1e-12);
    close(j.rotary[0], deg(90.0), 1e-12);
    assert!(frame_for_index(&m, &[f64::NAN]).is_err());
    assert!(frame_for_index(&m, &[]).is_err());
    assert!(frame_for_axis(&Machine::three_axis(), Vec3::X, Vec3::ZERO).is_err());
}

#[test]
fn a_turn_with_the_tip_almost_still_is_timed_smoothly() {
    // The tool turns 2° about a tip 20 mm off the A axis while the tip moves d: the time must
    // not drop a thousandfold once d passes a micrometre.
    let m = Machine::four_axis_a();
    let minutes = |d: f64| -> f64 {
        let path = Toolpath5 {
            moves: vec![
                Move5::Rapid { to: Pose::vertical(Vec3::new(0.0, 0.0, 20.0)) },
                Move5::Feed { to: Pose::new(Vec3::new(d, 0.0, 20.0), radial(deg(2.0))), feed: 300.0 },
            ],
        };
        solve(&m, &path, &SolveOptions::default())
            .unwrap()
            .iter()
            .map(|j| match j {
                JointMove::Feed { minutes, .. } => *minutes,
                _ => 0.0,
            })
            .sum()
    };
    // On the spot, the turn in degrees taken as millimetres.
    close(minutes(0.0), 2.0 / 300.0, 1e-12);
    let (a, b) = (minutes(0.0009), minutes(0.0011));
    assert!(a / b < 1.05 && b / a < 1.05, "{a} {b}");
    // Never faster than the travel of a point a millimetre from the tip, at any d.
    for d in [0.001, 0.005, 0.01, 0.02, 0.03] {
        assert!(minutes(d) >= deg(2.0) / 300.0 - 1e-15, "{d}");
        assert!(minutes(d) >= d / 300.0 - 1e-15, "{d}");
    }
    // A cut whose tip travels well past the turn is timed by its tip alone, as planned.
    close(minutes(5.0), Vec3::new(0.0, 0.0, 20.0).dist(Vec3::new(5.0, 0.0, 20.0)) / 300.0, 1e-12);
}

#[test]
fn big_four_axis_turns_stay_on_the_chord_all_along() {
    // Joint interpolation can swing the tip out and back across the planned line (an S near
    // the line at the middle): every piece is held within tolerance all along, not only at its
    // middle.
    let m = Machine::four_axis_a();
    let opts = SolveOptions::default();
    let unit = |v: Vec3| v.normalized().unwrap();
    let cases = [
        (
            Vec3::new(6.30261, -21.33960, 29.33622),
            unit(Vec3::new(0.0, -0.26090, 0.96536)),
            Vec3::new(-0.38149, -34.92762, 9.82827),
            unit(Vec3::new(0.0, -0.99957, -0.02917)),
        ),
        (Vec3::new(-1.716, -17.837, 21.734), radial(deg(-40.0)), Vec3::new(-1.539, -34.452, 8.264), radial(deg(-127.0))),
    ];
    for (t0, a0, t1, a1) in cases {
        let (p0, p1) = (Pose::new(t0, a0), Pose::new(t1, a1));
        let path = Toolpath5 { moves: vec![Move5::Rapid { to: p0 }, Move5::Feed { to: p1, feed: 300.0 }] };
        let out = solve(&m, &path, &opts).unwrap();
        let mut prev = out[0].joints().unwrap().clone();
        let mut worst: f64 = 0.0;
        for jm in &out[1..] {
            let j = jm.joints().unwrap();
            for k in 1..100 {
                let got = forward(&m, &prev.lerp(j, k as f64 / 100.0));
                worst = worst.max(got.tip.dist_to_segment(p0.tip, p1.tip));
            }
            prev = j.clone();
        }
        assert!(worst <= opts.tolerance * 1.05, "the tip leaves the line by {worst}");
        close3(forward(&m, &prev).tip, p1.tip, 1e-9);
    }
}
