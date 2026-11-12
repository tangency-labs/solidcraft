//! Tests of 4-axis rotary machining, against closed forms.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::foundation_tests::*;
use crate::*;

fn feeds() -> Feeds {
    Feeds { cut: 600.0, plunge: 150.0 }
}

/// Distance from the X axis.
fn radius(p: Vec3) -> f64 {
    p.y.hypot(p.z)
}

/// The angle of a radial tool axis, in `-π..=π`.
fn angle(axis: Vec3) -> f64 {
    axis.y.atan2(axis.z)
}

/// Every pose's axis is unit, radial and through the tip.
fn radial_axes(path: &Toolpath5) {
    for pose in path.poses() {
        close((pose.axis.len() - 1.0).abs(), 0.0, 1e-12);
        close(pose.axis.x, 0.0, 1e-12);
        let r = radius(pose.tip);
        if r > 1e-9 {
            close3(pose.axis, Vec3::new(0.0, pose.tip.y / r, pose.tip.z / r), 1e-9);
        }
    }
}

/// The nearest the straight move `a`–`b` comes to the X axis.
fn nearest_to_axis(a: Vec3, b: Vec3) -> f64 {
    let (p, q) = (Vec2::new(a.y, a.z), Vec2::new(b.y, b.z));
    let d = q - p;
    let t = if d.len2() > 0.0 { (-p.dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
    p.lerp(q, t).len()
}

#[test]
fn the_angle_convention() {
    close3(radial(0.0), Vec3::new(0.0, 0.0, 1.0), 1e-15);
    // Positive angles turn the tool from +Z towards +Y.
    close3(radial(FRAC_PI_2), Vec3::new(0.0, 1.0, 0.0), 1e-15);
    close3(radial(0.3), rotate(Vec3::new(0.0, 0.0, 1.0), Vec3::new(-1.0, 0.0, 0.0), 0.3), 1e-15);
    let p = rotary_pose(3.0, 0.4, 7.0);
    close3(p.tip, Vec3::new(3.0, 7.0 * 0.4f64.sin(), 7.0 * 0.4f64.cos()), 1e-12);
    close3(p.axis, radial(0.4), 1e-15);
}

#[test]
fn poses_are_at_the_angle_a_4th_axis_is_written_at() {
    use crate::kinematics::{Machine, inverse};
    let m = Machine::four_axis_a();
    for th in [0.0, 0.3, -0.7, 1.5, 2.9, -3.0] {
        let j = inverse(&m, &rotary_pose(5.0, th, 12.0), None).unwrap();
        close(j.rotary[0], th, 1e-9);
        // The table turned, the tip is straight under the vertical tool.
        close3(j.linear, Vec3::new(5.0, 0.0, 12.0), 1e-9);
    }
    // Along a helix the written angle goes on round with the pose angle.
    let s = cylinder_x(20.0, -30.0, 30.0, 128);
    let mut p = finish(RotaryPattern::Helix { pitch: 2.0 });
    p.range = Some([-4.0, 4.0]);
    let path = rotary_finish(&s, &ball(6.0), &p).unwrap();
    let mut prev: Option<crate::kinematics::Joints> = None;
    let mut turned = 0.0;
    let mut last: Option<f64> = None;
    for pose in path.poses() {
        let a = angle(pose.axis);
        turned += last.map_or(a, |l| (a - l + PI).rem_euclid(TAU) - PI);
        last = Some(a);
        let j = inverse(&m, &pose, prev.as_ref()).unwrap();
        close(j.rotary[0], turned, 1e-9);
        prev = Some(j);
    }
    close(turned, 4.0 * TAU, 1e-9);
}

#[test]
fn wrapping_a_quarter_turn() {
    let r = 10.0;
    let quarter = TAU * r / 4.0;
    let flat = Toolpath {
        moves: vec![
            Move::Rapid { to: Vec3::new(0.0, 0.0, 5.0) },
            Move::Feed { to: Vec3::new(0.0, 0.0, -1.0), feed: 100.0 },
            Move::Feed { to: Vec3::new(20.0, quarter, -1.0), feed: 300.0 },
            Move::Rapid { to: Vec3::new(20.0, quarter, 5.0) },
            Move::Rapid { to: Vec3::new(0.0, 0.0, 5.0) },
        ],
    };
    let tol = 0.001;
    let path = wrap(&flat, r, tol).unwrap();
    verify5(&path, DEFAULT_MAX_ANGLE_STEP).unwrap();
    radial_axes(&path);
    assert!(path.stats().max_axis_step <= DEFAULT_MAX_ANGLE_STEP + 1e-12);
    let cutting: Vec<Pose> =
        path.moves.iter().filter_map(|m| if let Move5::Feed { to, feed } = *m { (feed == 300.0).then_some(to) } else { None }).collect();
    assert!(cutting.len() >= 45, "a quarter turn in steps of 2° at most");
    // Every tip at radius R + z, and the cut ends a quarter turn round.
    for p in &cutting {
        close(radius(p.tip), r - 1.0, 1e-9);
    }
    let last = cutting.last().unwrap();
    close3(last.axis, radial(FRAC_PI_2), 1e-12);
    close3(last.tip, Vec3::new(20.0, r - 1.0, 0.0), 1e-9);
    let first = path.poses().next().unwrap();
    close(first.axis_angle(last), FRAC_PI_2, 1e-12);
    // The chords stay within the tolerance of the helix.
    let mut prev = first;
    for m in &path.moves {
        if let Move5::Feed { to, .. } = *m {
            assert!(nearest_to_axis(prev.tip, to.tip) >= radius(prev.tip).min(radius(to.tip)) - tol - 1e-12);
        }
        if let Some(to) = m.target() {
            prev = to;
        }
    }
    // The rapid back round at radius 15 never comes inside it.
    let mut prev = first;
    let mut round = 0;
    for m in &path.moves {
        if let Move5::Rapid { to } = *m
            && radius(prev.tip) >= 15.0 - 1e-9
            && radius(to.tip) >= 15.0 - 1e-9
        {
            assert!(nearest_to_axis(prev.tip, to.tip) >= 15.0 - 1e-9);
            round += 1;
        }
        if let Some(to) = m.target() {
            prev = to;
        }
    }
    assert!(round >= 45, "{round}");
    close3(path.poses().last().unwrap().tip, Vec3::new(0.0, 0.0, 15.0), 1e-12);
}

#[test]
fn wrapping_arcs_and_refusing_bad_input() {
    let r = 8.0;
    let flat = Toolpath {
        moves: vec![
            Move::Rapid { to: Vec3::new(5.0, 0.0, 3.0) },
            Move::Feed { to: Vec3::new(5.0, 0.0, -0.5), feed: 100.0 },
            Move::Arc { to: Vec3::new(-5.0, 0.0, -0.5), center: Vec2::new(0.0, 0.0), ccw: true, feed: 200.0 },
            Move::Dwell { seconds: 0.5 },
            Move::Rapid { to: Vec3::new(-5.0, 0.0, 3.0) },
        ],
    };
    let path = wrap_with_step(&flat, r, 0.002, 0.01).unwrap();
    verify5(&path, 0.01).unwrap();
    radial_axes(&path);
    let arc: Vec<Pose> = path.moves.iter().filter_map(|m| if let Move5::Feed { to, feed: 200.0 } = *m { Some(to) } else { None }).collect();
    assert!(arc.len() > 20);
    for p in &arc {
        close(radius(p.tip), r - 0.5, 1e-9);
    }
    // Half a circle of radius 5 about the flat origin: round to y = 5 (θ = 5/8) and back.
    let top = arc.iter().map(|p| angle(p.axis)).fold(f64::NEG_INFINITY, f64::max);
    close(top, 5.0 / r, 0.01);
    close3(arc.last().unwrap().tip, Vec3::new(-5.0, 0.0, r - 0.5), 1e-9);
    assert!(path.moves.iter().any(|m| matches!(m, Move5::Dwell { .. })));
    // Refused: no radius, a point past the axis, bad tolerances and steps.
    assert!(wrap(&flat, 0.0, 0.01).is_err());
    assert!(wrap(&flat, f64::NAN, 0.01).is_err());
    assert!(wrap(&flat, r, f64::NAN).is_err());
    assert!(wrap_with_step(&flat, r, 0.01, 0.0).is_err());
    assert!(wrap_with_step(&flat, r, 0.01, f64::INFINITY).is_err());
    let deep = Toolpath { moves: vec![Move::Rapid { to: Vec3::new(0.0, 0.0, 5.0) }, Move::Feed { to: Vec3::new(0.0, 0.0, -9.0), feed: 100.0 }] };
    assert!(wrap(&deep, r, 0.01).is_err());
    let nan = Toolpath { moves: vec![Move::Rapid { to: Vec3::new(0.0, f64::NAN, 5.0) }] };
    assert!(wrap(&nan, r, 0.01).is_err());
    // Many turns at once are split, within the move limit or refused.
    let long = Toolpath { moves: vec![Move::Rapid { to: Vec3::new(0.0, 0.0, 1.0) }, Move::Feed { to: Vec3::new(0.0, 1e9, 0.0), feed: 100.0 }] };
    assert!(matches!(wrap(&long, 1.0, 0.001), Err(CamError::TooLarge(_))));
}

fn finish(pattern: RotaryPattern) -> RotaryParams {
    RotaryParams {
        pattern,
        clearance: 40.0,
        retract: 25.0,
        floor: 0.0,
        feeds: feeds(),
        range: None,
        step: 1.0,
        tolerance: 0.01,
        stock_to_leave: 0.0,
        max_angle_step: DEFAULT_MAX_ANGLE_STEP,
    }
}

fn ball(d: f64) -> Tool {
    Tool::new(1, ToolKind::BallEndmill, d, 20.0)
}

#[test]
fn rotary_lines_on_a_cylinder() {
    let rc = 20.0;
    let s = cylinder_x(rc, -30.0, 30.0, 256);
    let chord = rc * (1.0 - (PI / 256.0).cos());
    let p = finish(RotaryPattern::Lines { angle_step: 0.3, zigzag: true });
    let path = rotary_finish(&s, &ball(6.0), &p).unwrap();
    verify5(&path, p.max_angle_step).unwrap();
    radial_axes(&path);
    let cuts: Vec<Pose> = path.moves.iter().filter_map(|m| if let Move5::Feed { to, .. } = *m { Some(to) } else { None }).collect();
    assert!(cuts.len() > 1000);
    for q in &cuts {
        let r = radius(q.tip);
        assert!(r <= rc + 1e-9 && r >= rc - chord - 1e-9, "{q:?}: {r}");
    }
    // 21 lines round (links between them turn at the ends, not half way along).
    let mut angles: Vec<f64> = cuts.iter().filter(|q| q.tip.x.abs() < 1e-9).map(|q| angle(q.axis).rem_euclid(TAU)).collect();
    angles.sort_by(f64::total_cmp);
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert_eq!(angles.len(), 21, "{angles:?}");
    // Zigzag lines on a cylinder link at feed: out only at the start and the end.
    let out = path.moves.iter().filter(|m| matches!(m, Move5::Rapid { .. })).count();
    assert!(out <= 4, "{out} rapids");
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(serde_json::from_str::<RotaryParams>(&json).unwrap(), p);
}

#[test]
fn a_rotary_helix_keeps_its_pitch() {
    let rc = 20.0;
    let s = cylinder_x(rc, -30.0, 30.0, 256);
    let chord = rc * (1.0 - (PI / 256.0).cos());
    let mut p = finish(RotaryPattern::Helix { pitch: 2.0 });
    p.range = Some([-10.0, 10.0]);
    let path = rotary_finish(&s, &ball(6.0), &p).unwrap();
    verify5(&path, p.max_angle_step).unwrap();
    radial_axes(&path);
    let cuts: Vec<Pose> = path.moves.iter().filter_map(|m| if let Move5::Feed { to, .. } = *m { Some(to) } else { None }).collect();
    // The angle goes on round: unwrapped, x advances the pitch each turn.
    let mut turned = 0.0;
    let mut last = angle(cuts[0].axis);
    for q in &cuts {
        let a = angle(q.axis);
        turned += (a - last + PI).rem_euclid(TAU) - PI;
        last = a;
        close(q.tip.x, -10.0 + 2.0 * turned / TAU, 1e-9);
        let r = radius(q.tip);
        assert!(r <= rc + 1e-9 && r >= rc - chord - 1e-9);
    }
    close(turned, 10.0 * TAU, 1e-9);
    // One continuous cut: in once, out once.
    assert_eq!(path.moves.iter().filter(|m| matches!(m, Move5::Rapid { .. })).count(), 3);
    assert!(path.stats().max_axis_step <= p.max_angle_step + 1e-12);
}

#[test]
fn rotary_finishing_a_sphere_never_cuts_into_it() {
    // A sphere on the axis, finished radially: the tool rests on it, and where it is past
    // the sphere it lifts out.
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), 12.0, 48);
    let mut p = finish(RotaryPattern::Lines { angle_step: 0.5, zigzag: true });
    p.range = Some([-16.0, 16.0]);
    p.stock_to_leave = 0.3;
    let tool = ball(4.0);
    let path = rotary_finish(&s, &tool, &p).unwrap();
    verify5(&path, p.max_angle_step).unwrap();
    radial_axes(&path);
    let c = tool.cutter().unwrap().grown(0.3).unwrap();
    for pose in path.points().iter().filter(|(_, cutting)| *cutting).map(|(p, _)| p) {
        let x = pose.tip.x;
        if let Some(d) = drop_along(&s, &c, Vec3::new(x, 0.0, 0.0), pose.axis).unwrap() {
            assert!(radius(pose.tip) >= d + 0.3 - 1e-9, "{pose:?} is in the part");
        }
    }
}

fn roughing(pattern: RotaryPattern) -> RotaryRoughingParams {
    RotaryRoughingParams {
        pattern,
        stock_radius: 15.0,
        clearance: 25.0,
        retract: 18.0,
        floor: 8.0,
        stepdown: 2.0,
        feeds: feeds(),
        range: Some([-20.0, 20.0]),
        step: 1.0,
        tolerance: 0.01,
        stock_to_leave: 0.5,
        max_angle_step: DEFAULT_MAX_ANGLE_STEP,
    }
}

#[test]
fn rotary_roughing_round_stock_down_to_a_cylinder() {
    let s = cylinder_x(10.0, -30.0, 30.0, 256);
    let chord = 10.0 * (1.0 - (PI / 256.0).cos());
    // Levels 13.25, 11.5, 9.75, 8: the tool cuts the first two, then the part (10 + 0.5
    // left) instead of the third, and nothing at the fourth.
    for pattern in [RotaryPattern::Lines { angle_step: 0.4, zigzag: true }, RotaryPattern::Helix { pitch: 3.0 }] {
        let p = roughing(pattern);
        let path = rotary_roughing(&s, &ball(6.0), &p).unwrap();
        verify5(&path, p.max_angle_step).unwrap();
        radial_axes(&path);
        let mut radii: Vec<f64> =
            path.moves.iter().filter_map(|m| if let Move5::Feed { to, .. } = *m { Some(radius(to.tip)) } else { None }).collect();
        assert!(radii.iter().all(|r| *r >= 10.5 - chord - 1e-9), "never into the part");
        radii.iter_mut().for_each(|r| *r = (*r * 1e3).round() / 1e3);
        radii.sort_by(f64::total_cmp);
        radii.dedup();
        let (lo, hi) = (radii[0], *radii.last().unwrap());
        assert!((10.5 - 0.002..=10.5).contains(&lo), "{radii:?}");
        assert!(radii.contains(&11.5) && hi == 13.25, "{radii:?}");
        assert!(!radii.iter().any(|r| (r - 9.75).abs() < 1e-3 || (r - 8.0).abs() < 1e-3));
    }
    let json = serde_json::to_string(&roughing(RotaryPattern::Helix { pitch: 3.0 })).unwrap();
    assert_eq!(serde_json::from_str::<RotaryRoughingParams>(&json).unwrap(), roughing(RotaryPattern::Helix { pitch: 3.0 }));
}

#[test]
fn hostile_rotary_params_are_refused() {
    let s = cylinder_x(10.0, -30.0, 30.0, 64);
    let tool = ball(6.0);
    let good = finish(RotaryPattern::Lines { angle_step: 0.5, zigzag: false });
    rotary_finish(&s, &tool, &good).unwrap();
    let bad: Vec<Box<dyn Fn(&mut RotaryParams)>> = vec![
        Box::new(|p| p.pattern = RotaryPattern::Lines { angle_step: 0.0, zigzag: true }),
        Box::new(|p| p.pattern = RotaryPattern::Lines { angle_step: f64::NAN, zigzag: true }),
        Box::new(|p| p.pattern = RotaryPattern::Helix { pitch: 0.0 }),
        Box::new(|p| p.pattern = RotaryPattern::Helix { pitch: f64::NAN }),
        Box::new(|p| p.clearance = 20.0),
        Box::new(|p| p.retract = f64::NAN),
        Box::new(|p| p.retract = 9.0),
        Box::new(|p| p.floor = -1.0),
        Box::new(|p| p.floor = 30.0),
        Box::new(|p| p.range = Some([5.0, 5.0])),
        Box::new(|p| p.range = Some([f64::NAN, 5.0])),
        Box::new(|p| p.step = 0.0),
        Box::new(|p| p.tolerance = -1.0),
        Box::new(|p| p.stock_to_leave = f64::NAN),
        Box::new(|p| p.max_angle_step = 0.0),
        Box::new(|p| p.max_angle_step = 2.0),
        Box::new(|p| p.feeds.plunge = f64::NAN),
    ];
    for (i, f) in bad.iter().enumerate() {
        let mut p = good.clone();
        f(&mut p);
        assert!(matches!(rotary_finish(&s, &tool, &p), Err(CamError::Invalid(_))), "case {i}");
    }
    let mut huge = good.clone();
    huge.step = 1e-4;
    assert!(matches!(rotary_finish(&s, &tool, &huge), Err(CamError::TooLarge(_))));
    let mut fine = good.clone();
    fine.pattern = RotaryPattern::Helix { pitch: 1e-4 };
    assert!(matches!(rotary_finish(&s, &tool, &fine), Err(CamError::TooLarge(_))));
    let mut r = roughing(RotaryPattern::Helix { pitch: 3.0 });
    r.stock_radius = 20.0;
    assert!(rotary_roughing(&s, &tool, &r).is_err(), "stock outside the retract radius");
    r.stock_radius = 15.0;
    r.stepdown = 0.0;
    assert!(rotary_roughing(&s, &tool, &r).is_err());
    r.stepdown = 2.0;
    r.floor = 16.0;
    assert!(rotary_roughing(&s, &tool, &r).is_err());
}

fn flat(d: f64) -> Tool {
    Tool::new(1, ToolKind::FlatEndmill, d, 20.0)
}

/// Both surfaces' triangles as one surface.
fn joined(a: &Surface, b: &Surface) -> Surface {
    Surface::new(a.triangles().iter().chain(b.triangles()).copied().collect()).unwrap()
}

/// The deepest any cutting move of `path` goes into the part, `leave` grown on it: each feed
/// is followed in short pieces, and at each the cutter is dropped radially through it.
fn deepest_gouge(path: &Toolpath5, s: &Surface, tool: &Tool, leave: f64) -> f64 {
    let c = tool.cutter().unwrap().grown(leave).unwrap();
    let mut worst = f64::NEG_INFINITY;
    let mut prev: Option<Vec3> = None;
    for m in &path.moves {
        if let (Move5::Feed { to, .. }, Some(from)) = (*m, prev) {
            let n = (from.dist(to.tip) / 0.05).ceil().clamp(1.0, 10_000.0) as usize;
            for k in 0..=n {
                let p = from.lerp(to.tip, k as f64 / n as f64);
                let th = p.y.atan2(p.z);
                if let Some(d) = drop_along(s, &c, Vec3::new(p.x, 0.0, 0.0), radial(th)).unwrap() {
                    worst = worst.max(d + leave - radius(p));
                }
            }
        }
        if let Some(to) = m.target() {
            prev = Some(to.tip);
        }
    }
    worst
}

#[test]
fn a_fin_between_samples_is_not_cut_through() {
    // A shaft of radius 10 with a 0.5 mm fin of radius 12 at x 2.1..2.6 (its edges off the
    // places samples land, where the cutter would only graze them). Samples 10 mm apart land at
    // x 0, 5 and 10, all on the shaft, and the tool would feed straight through the fin.
    let s = joined(&cylinder_x(10.0, -5.0, 15.0, 64), &cylinder_x(12.0, 2.1, 2.6, 64));
    let tool = flat(2.0);
    let mut p = finish(RotaryPattern::Lines { angle_step: 0.5, zigzag: true });
    p.range = Some([0.0, 10.0]);
    p.step = 10.0;
    let path = rotary_finish(&s, &tool, &p).unwrap();
    verify5(&path, p.max_angle_step).unwrap();
    let gouge = deepest_gouge(&path, &s, &tool, 0.0);
    assert!(gouge <= 0.05, "the finish cuts {gouge} mm into the fin");
    // Roughing: a 1 mm flange between two samples that see nothing under the tool.
    let s = cylinder_x(10.0, 4.5, 5.5, 64);
    let tool = flat(2.0);
    let mut r = roughing(RotaryPattern::Lines { angle_step: 0.13, zigzag: true });
    r.range = Some([-2.6, 12.4]);
    r.step = 5.0;
    let path = rotary_roughing(&s, &tool, &r).unwrap();
    verify5(&path, r.max_angle_step).unwrap();
    let gouge = deepest_gouge(&path, &s, &tool, r.stock_to_leave);
    assert!(gouge <= 0.05, "the roughing cuts {gouge} mm into the flange");
}

#[test]
fn rotary_roughing_refuses_passes_too_far_apart_for_the_tool() {
    let s = cylinder_x(10.0, -30.0, 30.0, 64);
    let tool = flat(6.0);
    // Lines 0.9 radians apart are 13.4 mm apart round the stock of radius 15, and a helix of
    // pitch 10: both would leave ridges of stock at full height between passes.
    for pattern in [RotaryPattern::Lines { angle_step: 0.9, zigzag: true }, RotaryPattern::Helix { pitch: 10.0 }] {
        let r = roughing(pattern);
        assert!(matches!(rotary_roughing(&s, &tool, &r), Err(CamError::Invalid(_))), "{pattern:?}");
        // Finishing only leaves larger scallops: accepted.
        let mut p = finish(pattern);
        p.range = Some([-5.0, 5.0]);
        rotary_finish(&s, &tool, &p).unwrap();
    }
    // One line round, or two on stock wider than the tool, never clear the stock either.
    for angle_step in [TAU, PI] {
        let r = roughing(RotaryPattern::Lines { angle_step, zigzag: false });
        assert!(matches!(rotary_roughing(&s, &tool, &r), Err(CamError::Invalid(_))), "{angle_step}");
    }
    // At most the tool diameter apart: accepted.
    for pattern in [RotaryPattern::Lines { angle_step: 0.39, zigzag: true }, RotaryPattern::Helix { pitch: 6.0 }] {
        rotary_roughing(&s, &tool, &roughing(pattern)).unwrap();
    }
}

#[test]
fn a_roughing_helix_clears_the_ends_of_the_range() {
    // Round the whole turn, at both ends of the range, some cut runs at that end.
    let s = cylinder_x(10.0, -30.0, 30.0, 128);
    let r = roughing(RotaryPattern::Helix { pitch: 5.0 });
    let path = rotary_roughing(&s, &flat(6.0), &r).unwrap();
    verify5(&path, r.max_angle_step).unwrap();
    radial_axes(&path);
    let cuts: Vec<Pose> = path
        .moves
        .iter()
        .filter_map(|m| if let Move5::Feed { to, .. } = *m { Some(to) } else { None })
        .filter(|q| radius(q.tip) < r.stock_radius - 1.0)
        .collect();
    for x in [-20.0, 20.0] {
        for k in 0..64 {
            let th = TAU * k as f64 / 64.0;
            let near = cuts.iter().any(|q| (q.tip.x - x).abs() <= 0.5 && ((angle(q.axis) - th + PI).rem_euclid(TAU) - PI).abs() <= 0.05);
            assert!(near, "no cut at x {x}, angle {th}");
        }
    }
    // The tool never leaves the range.
    assert!(cuts.iter().all(|q| (-20.0 - 1e-9..=20.0 + 1e-9).contains(&q.tip.x)));
}

#[test]
fn rotary_roughing_refuses_too_many_levels_times_samples() {
    // 10,000 levels over about 50,000 samples: refused before the levels are walked, which
    // would take long and, the part being wholly outside the stock, cut nothing.
    let s = cylinder_x(12.0, -30.0, 30.0, 64);
    let mut r = roughing(RotaryPattern::Lines { angle_step: 0.05, zigzag: true });
    r.stock_radius = 10.0;
    r.clearance = 13.0;
    r.retract = 12.5;
    r.floor = 9.0;
    r.stepdown = 1e-4;
    r.range = Some([-20.0, 20.0]);
    r.step = 0.1;
    r.stock_to_leave = 0.0;
    assert!(matches!(rotary_roughing(&s, &ball(6.0), &r), Err(CamError::TooLarge(_))));
    // With a few levels the part, wholly outside the stock, leaves nothing to cut.
    r.stepdown = 0.1;
    let path = rotary_roughing(&s, &ball(6.0), &r).unwrap();
    assert!(!path.moves.iter().any(|m| matches!(m, Move5::Feed { .. })));
}

/// Going in towards the axis (into the material), no cutting move goes faster than the plunge
/// feed: a ramp down the part's outline on a later level is slowed in proportion.
#[test]
fn rotary_descents_never_go_in_faster_than_the_plunge_feed() {
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), 12.0, 48);
    let f = feeds();
    for pattern in [RotaryPattern::Lines { angle_step: 0.3, zigzag: true }, RotaryPattern::Helix { pitch: 4.0 }] {
        let mut p = roughing(pattern);
        p.range = Some([-10.0, 10.0]);
        let path = rotary_roughing(&s, &Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0), &p).unwrap();
        let mut pos: Option<Vec3> = None;
        let mut inward = 0;
        for m in &path.moves {
            if let (Move5::Feed { to, feed }, Some(q)) = (*m, pos) {
                let (len, drop) = (q.dist(to.tip), radius(q) - radius(to.tip));
                if drop > 1e-9 && len > 0.0 {
                    inward += 1;
                    assert!(feed * drop / len <= f.plunge * (1.0 + 1e-9), "in at {} mm/min", feed * drop / len);
                }
            }
            if let Some(t) = m.target() {
                pos = Some(t.tip);
            }
        }
        assert!(inward > 0);
    }
}

/// The rings of a path: its cuts grouped by X in order, as `(x, angles)`, without the links
/// (the few cuts across X between two rings).
fn rings_of(path: &Toolpath5) -> Vec<(f64, Vec<f64>)> {
    let mut rings: Vec<(f64, Vec<f64>)> = Vec::new();
    for m in &path.moves {
        let Move5::Feed { to, .. } = *m else { continue };
        match rings.last_mut() {
            Some((x, angles)) if (to.tip.x - *x).abs() <= 1e-9 => angles.push(angle(to.axis)),
            _ => rings.push((to.tip.x, vec![angle(to.axis)])),
        }
    }
    rings.retain(|(_, a)| a.len() > 10);
    rings
}

/// Rings: a full turn at each X, exactly the stepover apart when it divides the range, every
/// other one turning back with zigzag, all the same way without.
#[test]
fn rotary_rings_step_over_exactly_along_x() {
    let rc = 20.0;
    let s = cylinder_x(rc, -30.0, 30.0, 256);
    let chord = rc * (1.0 - (PI / 256.0).cos());
    for zigzag in [true, false] {
        let mut p = finish(RotaryPattern::Rings { stepover: 2.5, zigzag });
        p.range = Some([-10.0, 10.0]);
        let path = rotary_finish(&s, &ball(6.0), &p).unwrap();
        verify5(&path, p.max_angle_step).unwrap();
        radial_axes(&path);
        assert!(path.stats().max_axis_step <= p.max_angle_step + 1e-12);
        for q in path.poses().filter(|q| radius(q.tip) < 24.0) {
            let r = radius(q.tip);
            assert!(r <= rc + 1e-9 && r >= rc - chord - 1e-9, "{q:?}: {r}");
        }
        // 9 rings, 2.5 mm apart, both ends of the range included.
        let rings = rings_of(&path);
        let xs: Vec<f64> = rings.iter().map(|(x, _)| *x).collect();
        assert_eq!(xs.len(), 9, "{xs:?}");
        for (k, x) in xs.iter().enumerate() {
            close(*x, -10.0 + 2.5 * k as f64, 1e-9);
        }
        // Each ring goes a whole turn, one way or the other.
        let mut ways = Vec::new();
        for (_, a) in &rings {
            let turned: f64 = a.windows(2).map(|w| (w[1] - w[0] + PI).rem_euclid(TAU) - PI).sum();
            assert!(turned.abs() >= TAU - 2.0 * p.max_angle_step, "{turned}");
            ways.push(turned.signum());
        }
        if zigzag {
            assert!(ways.windows(2).all(|w| w[0] == -w[1]), "{ways:?}");
        } else {
            assert!(ways.iter().all(|w| *w == ways[0]), "{ways:?}");
        }
        // Linked at feed: in once at the start, out at the end.
        assert!(path.moves.iter().filter(|m| matches!(m, Move5::Rapid { .. })).count() <= 4);
    }
    // A stepover that does not divide the range: evenly spaced, no more than it apart.
    let mut p = finish(RotaryPattern::Rings { stepover: 3.0, zigzag: true });
    p.range = Some([-10.0, 10.0]);
    let rings = rings_of(&rotary_finish(&s, &ball(6.0), &p).unwrap());
    assert_eq!(rings.len(), 8);
    for (k, (x, _)) in rings.iter().enumerate() {
        close(*x, -10.0 + 20.0 * k as f64 / 7.0, 1e-9);
    }
    // Roughing refuses rings farther apart than the tool is wide, and roughs with closer ones.
    let mut r = roughing(RotaryPattern::Rings { stepover: 7.0, zigzag: true });
    assert!(rotary_roughing(&s, &ball(6.0), &r).is_err());
    r.pattern = RotaryPattern::Rings { stepover: 3.0, zigzag: false };
    let rough = rotary_roughing(&cylinder_x(10.0, -30.0, 30.0, 128), &Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0), &r).unwrap();
    verify5(&rough, r.max_angle_step).unwrap();
    assert!(rough.poses().all(|q| radius(q.tip) >= 10.0 + 0.5 - 0.02), "the stock to leave is left");
    for stepover in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e300] {
        let p = finish(RotaryPattern::Rings { stepover, zigzag: true });
        assert!(rotary_finish(&s, &ball(6.0), &p).is_err(), "{stepover}");
    }
    // Rings too fine to sample are refused, not run.
    let mut p = finish(RotaryPattern::Rings { stepover: 1e-4, zigzag: true });
    p.step = 1e-4;
    assert!(matches!(rotary_finish(&s, &ball(6.0), &p), Err(CamError::TooLarge(_))));
    let json = serde_json::to_string(&RotaryPattern::Rings { stepover: 2.0, zigzag: false }).unwrap();
    assert_eq!(json, r#"{"type":"rings","stepover":2.0,"zigzag":false}"#);
}

/// The profile `(x, r)` turned about the X axis in `n` steps.
fn turned_x(profile: &[(f64, f64)], n: usize) -> Surface {
    let at = |x: f64, r: f64, k: usize| Vec3::new(x, 0.0, 0.0) + radial(TAU * k as f64 / n as f64) * r;
    let mut tris = Vec::new();
    for w in profile.windows(2) {
        let (a, b) = (w[0], w[1]);
        for k in 0..n {
            tris.push([at(a.0, a.1, k), at(b.0, b.1, k), at(b.0, b.1, k + 1)]);
            tris.push([at(a.0, a.1, k), at(b.0, b.1, k + 1), at(a.0, a.1, k + 1)]);
        }
    }
    Surface::new(tris).unwrap()
}

/// A spindle: r 14 to a shoulder at X -8, down its face to an R3 fillet into r 10. A ⌀4 ball
/// on the ring at X -6 just reaches the shoulder's face with its side: it rests in the fillet
/// (radius 11), and the mesh's facets, each within its tessellation error of the true surface,
/// must not hold it up by up to a millimetre at some angles and not at others (a ring that
/// dips in and out thousands of times). It runs round at one radius, its ball never more than
/// that error into the mesh.
#[test]
fn a_ring_beside_a_shoulder_runs_round_at_one_radius() {
    let tol = 0.01;
    let chord = 0.25 * tol;
    // The fillet in steps whose chords are within the tessellation error of the arc.
    let steps = (FRAC_PI_2 / (2.0 * (1.0f64 - chord / 3.0).acos())).ceil() as usize;
    let mut profile = vec![(-30.0, 0.0), (-30.0, 14.0), (-8.0, 14.0), (-8.0, 13.0)];
    profile.extend((1..steps).map(|k| {
        let a = PI + FRAC_PI_2 * k as f64 / steps as f64;
        (-5.0 + 3.0 * a.cos(), 13.0 + 3.0 * a.sin())
    }));
    profile.extend([(-5.0, 10.0), (10.0, 10.0), (10.0, 0.0)]);
    let n = (PI / (1.0f64 - chord / 14.0).acos()).ceil() as usize;
    let s = turned_x(&profile, n);
    let tool = ball(4.0);
    let mut p = finish(RotaryPattern::Rings { stepover: 1.0, zigzag: false });
    p.range = Some([-7.0, -5.0]);
    p.tolerance = tol;
    let path = rotary_finish(&s, &tool, &p).unwrap();
    // The ring's own moves: at X -6, each turning A on from the move before (the link in to it
    // comes down radially, at one angle).
    let ring: Vec<Pose> = path
        .moves
        .windows(2)
        .filter_map(|w| match (w[0], w[1]) {
            (Move5::Feed { to: from, .. }, Move5::Feed { to, .. })
                if (to.tip.x + 6.0).abs() < 1e-9 && (from.tip.x + 6.0).abs() < 1e-9 && radius(to.tip) < 12.5 && from.axis.dist(to.axis) > 1e-9 =>
            {
                Some(to)
            }
            _ => None,
        })
        .collect();
    assert!(ring.len() >= 150, "{}", ring.len());
    let radii: Vec<f64> = ring.iter().map(|q| radius(q.tip)).collect();
    let (lo, hi) = radii.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), r| (a.min(*r), b.max(*r)));
    assert!(hi - lo <= tol, "the ring's radius runs from {lo} to {hi}");
    assert!((lo - 11.0).abs() <= 0.1, "{lo}");
    // A whole turn in no more moves than the angle step asks for.
    assert!(ring.len() <= 200, "{} moves on the ring", ring.len());
    // Never more than the tessellation error into the mesh, measured from the ball's centre.
    for q in path.poses() {
        let centre = q.tip + q.axis * 2.0;
        let d = crate::sim::distance_within(&s, centre, 2.0);
        assert!(d >= 2.0 - chord - 1e-6, "{q:?}: the ball is {} mm into the part", 2.0 - d);
    }
}

/// A ring whose ball is exactly one radius from the part's end face (a range that starts a tool
/// radius beyond the free end, as one is told to give it) only touches the face's edge with its
/// side. It must run round at the edge's radius, not dive in and out towards the axis where the
/// facets of the end face happen to be found or missed at that tangency (which once made one
/// ring 12 000 moves and an hour long).
#[test]
fn a_ring_a_tool_radius_off_the_end_runs_round_at_the_edge() {
    let s = turned_x(&[(2.0, 0.0), (2.0, 12.0), (20.0, 12.0), (20.0, 0.0)], 720);
    let tool = ball(4.0);
    let mut p = finish(RotaryPattern::Rings { stepover: 0.5, zigzag: false });
    p.range = Some([0.0, 4.0]);
    p.retract = 16.0;
    p.clearance = 20.0;
    let path = rotary_finish(&s, &tool, &p).unwrap();
    let end: Vec<f64> = path
        .moves
        .iter()
        .filter_map(|m| match *m {
            Move5::Feed { to, .. } if to.tip.x.abs() < 1e-9 => Some(radius(to.tip)),
            _ => None,
        })
        .collect();
    let lowest = end.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(lowest >= 10.0 - 0.01, "the ring at the end dives to radius {lowest}");
    assert!(end.len() <= 400, "{} moves on the ring at the end", end.len());
    let feeds = path.moves.iter().filter(|m| matches!(m, Move5::Feed { .. })).count();
    let with = rotary_finish(&s, &tool, &RotaryParams { stock_to_leave: 0.012, ..p }).unwrap();
    let feeds_with = with.moves.iter().filter(|m| matches!(m, Move5::Feed { .. })).count();
    assert!(feeds as f64 <= 1.2 * feeds_with as f64, "{feeds} moves at no stock left, {feeds_with} at 0.012");
}

/// Rings with no stock left over a bar with a flat stay within the gouge check's allowance of
/// the part as tessellated (the tolerance less the tessellation's share, a quarter of it): at
/// the flat's edges the rings rise over a sharp edge, and the chords between their samples, on
/// top of the steadied rest beside steep faces, must not cut deeper than that.
#[test]
fn rings_with_no_stock_left_stay_within_the_gouge_allowance() {
    let s = flatted_bar(12.0, 9.0, -15.0, 15.0);
    let tool = ball(4.0);
    let tol = 0.01;
    let mut p = finish(RotaryPattern::Rings { stepover: 0.5, zigzag: false });
    p.range = Some([-17.0, 17.0]);
    p.retract = 16.0;
    p.clearance = 20.0;
    p.tolerance = tol;
    let path = rotary_finish(&s, &tool, &p).unwrap();
    let hits = crate::collision::gouges(&s, &tool.cutter().unwrap(), &path, 0.75 * tol, 0.05).unwrap();
    let worst = hits.iter().map(|h| h.distance).fold(0.0, f64::max);
    assert!(hits.is_empty(), "{} moves cut up to {worst} mm into the part (allowed {})", hits.len(), 0.75 * tol);
}
