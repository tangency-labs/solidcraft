//! Tests of the multi-axis collision checks and of tilting a toolpath clear.

use std::f64::consts::{PI, TAU};

use solidcore_record::Vec3;

use crate::collision::*;
use crate::foundation_tests::*;
use crate::*;

fn deg(d: f64) -> f64 {
    d * PI / 180.0
}

fn path3(points: &[(Vec3, bool)]) -> Toolpath5 {
    let moves = points
        .iter()
        .map(|&(p, cut)| if cut { Move5::Feed { to: Pose::vertical(p), feed: 500.0 } } else { Move5::Rapid { to: Pose::vertical(p) } })
        .collect();
    Toolpath5 { moves }
}

#[test]
fn a_path_through_a_block_gouges_and_a_dropped_one_does_not() {
    let b = block(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 5.0));
    let ball = Cutter::ball(3.0).unwrap();
    let through = path3(&[(Vec3::new(-20.0, 0.0, 20.0), false), (Vec3::new(-20.0, 0.0, 3.0), true), (Vec3::new(20.0, 0.0, 3.0), true)]);
    let hits = gouges(&b, &ball, &through, 0.01, 0.5).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    let h = hits[0];
    assert_eq!((h.index, h.envelope), (2, None));
    // On the top the tip must be at 5: 2 mm farther out.
    close(h.distance, 2.0, 1e-9);
    assert!(h.pose.tip.x.abs() <= 10.0 + 1e-9);

    // Across the top with every point dropped onto the block (sampled finely: over the edges
    // the ball rolls round a circle of its radius, and the chords of that cut in by their
    // sagitta, 3·(1 − cos(0.25/3)) < 0.01).
    let mut pts = vec![(Vec3::new(-12.0, 0.0, 20.0), false)];
    for i in 0..=96 {
        let x = -12.0 + 0.25 * i as f64;
        pts.push((Vec3::new(x, 0.0, drop_z(&b, &ball, x, 0.0).unwrap()), true));
    }
    let dropped = path3(&pts);
    assert!(gouges(&b, &ball, &dropped, 0.01, 0.1).unwrap().is_empty());
    // Rapids are not checked: a rapid through the block is the links' business.
    let rapid = path3(&[(Vec3::new(-20.0, 0.0, 3.0), false), (Vec3::new(20.0, 0.0, 3.0), false)]);
    assert!(gouges(&b, &ball, &rapid, 0.01, 0.5).unwrap().is_empty());
}

#[test]
fn rotary_poses_round_a_cylinder() {
    // A ball held radially to a cylinder about X, as a rotary axis turns the part under it.
    let (rc, r) = (20.0, 3.0);
    let cyl = cylinder_x(rc, -50.0, 50.0, 256);
    let ball = Cutter::ball(r).unwrap();
    let radial = |phi: f64, x: f64, rho: f64| {
        let d = Vec3::new(0.0, phi.cos(), phi.sin());
        Pose::new(Vec3::new(x, 0.0, 0.0) + d * rho, d)
    };
    let around = |rho: f64| {
        let mut moves = vec![Move5::Rapid { to: radial(0.0, 0.0, rc + 10.0) }];
        for k in 0..=72 {
            let phi = TAU * k as f64 / 72.0;
            moves.push(Move5::Feed { to: radial(phi, 5.0 * phi, rho), feed: 400.0 });
        }
        Toolpath5 { moves }
    };
    // On the cylinder: the chords between poses 5° apart are inside it by 20·(1 − cos 2.5°).
    let sag = rc * (1.0 - deg(2.5).cos());
    assert!(gouges(&cyl, &ball, &around(rc), sag + 0.005, 0.5).unwrap().is_empty());
    let deep = gouges(&cyl, &ball, &around(rc - 1.0), 0.05, 0.5).unwrap();
    assert_eq!(deep.len(), 73, "every move cuts 1 mm in");
    for h in &deep {
        assert!((1.0 - 0.01..=1.0 + sag + 0.01).contains(&h.distance), "{h:?}");
    }
    // A short tool with a wide holder, radially over the cylinder, its holder clear of it.
    let mut tool = Tool::new(1, ToolKind::BallEndmill, 6.0, 10.0);
    tool.stickout = Some(12.0);
    tool.holder = vec![HolderSection { diameter: 30.0, length: 30.0 }];
    assert!(holder_collisions(&cyl, &tool, &around(rc), 1.0, 1.0).unwrap().is_empty());
    // Tilted 70° off radial about the ball's centre, the holder's rim comes down onto the
    // cylinder: its face's centre 9 up the axis from the ball's centre, 23 off the cylinder's
    // axis, puts the rim's nearest point 18.1 off it.
    let tilt = |p: Pose| {
        let a = rotate(p.axis, Vec3::new(1.0, 0.0, 0.0), deg(70.0));
        Pose::new(p.up(r) - a * r, a)
    };
    let leant = Toolpath5 {
        moves: around(rc).moves.iter().map(|m| if let Move5::Feed { to, feed } = *m { Move5::Feed { to: tilt(to), feed } } else { *m }).collect(),
    };
    let hits = holder_collisions(&cyl, &tool, &leant, 1.0, 1.0).unwrap();
    assert!(!hits.is_empty() && hits.iter().all(|h| h.envelope == Some(1)), "{hits:?}");
}

/// A floor at z = 0 with two long walls 30 high, at x ≤ −10 and x ≥ 30: a deep pocket.
fn pocket() -> Surface {
    let mut tris = Vec::new();
    for b in [
        block(Vec3::new(-50.0, -50.0, -20.0), Vec3::new(50.0, 50.0, 0.0)),
        block(Vec3::new(-20.0, -50.0, 0.0), Vec3::new(-10.0, 50.0, 30.0)),
        block(Vec3::new(30.0, -50.0, 0.0), Vec3::new(40.0, 50.0, 30.0)),
    ] {
        tris.extend_from_slice(b.triangles());
    }
    Surface::new(tris).unwrap()
}

/// A 6 mm ball 25 mm out of a 30 mm holder.
fn short_tool() -> Tool {
    let mut t = Tool::new(1, ToolKind::BallEndmill, 6.0, 10.0);
    t.stickout = Some(25.0);
    t.holder = vec![HolderSection { diameter: 30.0, length: 40.0 }];
    t
}

/// Down into the pocket 4 mm from the left wall, along it, and out.
fn along_the_wall(x: f64) -> Toolpath5 {
    path3(&[(Vec3::new(x, -20.0, 60.0), false), (Vec3::new(x, -20.0, 0.0), true), (Vec3::new(x, 20.0, 0.0), true), (Vec3::new(x, 20.0, 60.0), true)])
}

fn options(max_tilt: f64) -> TiltOptions {
    TiltOptions { max_tilt, step: deg(1.0), about: TiltAbout::FeedSide, clearance: 0.5, max_axis_step: deg(2.0), sample_step: 1.0, tolerance: 0.01 }
}

#[test]
fn the_holder_hits_the_wall_of_a_deep_pocket() {
    let s = pocket();
    let tool = short_tool();
    let path = along_the_wall(-6.0);
    let ball = tool.cutter().unwrap();
    assert!(gouges(&s, &ball, &path, 1e-6, 0.5).unwrap().is_empty(), "the ball itself is clear of the floor and the wall");
    let hits = holder_collisions(&s, &tool, &path, 0.5, 1.0).unwrap();
    // The shank (radius 3, 4 from the wall) is clear; the holder (radius 15) crosses the wall.
    assert!(hits.iter().all(|h| h.envelope == Some(1)), "{hits:?}");
    let along = hits.iter().find(|h| h.index == 2).unwrap();
    // The wall is 4 from the axis, the holder's radius 15.
    close(along.distance, 4.0 - 15.0, 1e-9);
    assert!(hits.iter().any(|h| h.index == 1) && hits.iter().any(|h| h.index == 3));
    // In the middle of the pocket, 20 from the left wall and 20 from the right, it is clear:
    // the holder's flat face is 25 above the floor, so the cylinder (not a capsule) is checked.
    assert!(holder_collisions(&s, &tool, &along_the_wall(10.0), 0.5, 1.0).unwrap().is_empty());
}

#[test]
fn tilting_clears_the_holder() {
    let s = pocket();
    let tool = short_tool();
    let path = along_the_wall(-6.0);
    for about in [
        TiltAbout::FeedSide,
        TiltAbout::TowardPoint { point: Vec3::new(1000.0, 0.0, 80.0) },
        TiltAbout::AwayFromPoint { point: Vec3::new(-1000.0, 0.0, 0.0) },
    ] {
        let opts = TiltOptions { about, ..options(deg(40.0)) };
        let out = tilt_away(&s, &tool, &path, &opts).unwrap();
        let h = holder_collisions(&s, &tool, &out, opts.clearance, 0.5).unwrap();
        assert!(h.is_empty(), "{about:?} {h:?}");
        assert!(gouges(&s, &tool.cutter().unwrap(), &out, opts.tolerance, 0.5).unwrap().is_empty());
        verify5(&out, opts.max_axis_step).unwrap();
        let stats = out.stats();
        assert!(stats.max_axis_step <= opts.max_axis_step + 1e-9);
        // The ball turns about its centre: the holder's rim must be 9.5 from the axis's foot
        // 4 from the wall, 22 up from the centre: 22·sin θ − 15·cos θ > −3.5, so θ ≥ 27° (in
        // whole degrees), and one degree more is kept in hand.
        assert!(stats.max_tilt >= deg(28.0) - 1e-9 && stats.max_tilt <= deg(29.5), "{about:?}: {}", stats.max_tilt.to_degrees());
        for p in out.poses() {
            assert!(p.axis.x >= -1e-12, "{about:?}: it leans away from the wall, to +X: {p:?}");
            // The ball's centre stays on the line it followed.
            close(p.up(3.0).x, -6.0, 1e-9);
        }
        // Along the wall, the ball still sits on the floor.
        let floor: Vec<Pose> = out.poses().filter(|p| p.up(3.0).z.abs() - 3.0 <= 1e-9 && p.up(3.0).y.abs() < 19.0).collect();
        assert!(!floor.is_empty());
        for p in &floor {
            close(p.up(3.0).z, 3.0, 1e-9);
            assert!(p.axis.z.acos() >= deg(27.0) - 1e-9);
        }
    }
    // A tilt of at most 15° cannot clear it: refused, naming the move.
    match tilt_away(&s, &tool, &path, &options(deg(15.0))) {
        Err(CamError::Unsafe(why)) => assert!(why.starts_with("move "), "{why}"),
        other => panic!("{other:?}"),
    }
    // Nothing collides: nothing changes but the division into short moves.
    let clear = tilt_away(&s, &tool, &along_the_wall(10.0), &options(deg(40.0))).unwrap();
    assert_eq!(clear.stats().max_tilt, 0.0);
}

#[test]
fn hostile_collision_inputs_are_refused() {
    let s = pocket();
    let tool = short_tool();
    let ball = tool.cutter().unwrap();
    let path = along_the_wall(-6.0);
    for step in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(gouges(&s, &ball, &path, 0.01, step).is_err());
        assert!(holder_collisions(&s, &tool, &path, 0.5, step).is_err());
        assert!(tilt_away(&s, &tool, &path, &TiltOptions { sample_step: step, ..options(deg(40.0)) }).is_err());
    }
    assert!(gouges(&s, &ball, &path, f64::NAN, 1.0).is_err());
    assert!(holder_collisions(&s, &tool, &path, f64::NAN, 1.0).is_err());
    let bad = Toolpath5 { moves: vec![Move5::Rapid { to: Pose::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)) }] };
    assert!(gouges(&s, &ball, &bad, 0.01, 1.0).is_err());
    let unnormal = Toolpath5 { moves: vec![Move5::Rapid { to: Pose::new(Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0)) }] };
    assert!(holder_collisions(&s, &tool, &unnormal, 0.5, 1.0).is_err());
    for o in [
        TiltOptions { max_tilt: f64::NAN, ..options(deg(40.0)) },
        TiltOptions { max_tilt: deg(100.0), ..options(deg(40.0)) },
        TiltOptions { step: 0.0, ..options(deg(40.0)) },
        TiltOptions { max_axis_step: 0.0, ..options(deg(40.0)) },
        TiltOptions { clearance: -1.0, ..options(deg(40.0)) },
        TiltOptions { about: TiltAbout::TowardPoint { point: Vec3::new(f64::NAN, 0.0, 0.0) }, ..options(deg(40.0)) },
    ] {
        assert!(tilt_away(&s, &tool, &path, &o).is_err(), "{o:?}");
    }
    assert!(tilt_away(&s, &Tool::new(1, ToolKind::Drill, 6.0, 30.0), &path, &options(deg(40.0))).is_err());
    // A tool too long a way round to sample is refused, not looped over.
    let far = path3(&[(Vec3::ZERO, false), (Vec3::new(1.0e6, 0.0, 0.0), true)]);
    assert!(matches!(gouges(&s, &ball, &far, 0.01, 1e-3), Err(CamError::TooLarge(_))));
    // An empty surface has nothing to hit.
    let empty = Surface::new(vec![]).unwrap();
    assert!(gouges(&empty, &ball, &path, 0.01, 1.0).unwrap().is_empty());
    assert!(holder_collisions(&empty, &tool, &path, 0.5, 1.0).unwrap().is_empty());
    let json = serde_json::to_string(&options(0.5)).unwrap();
    assert_eq!(serde_json::from_str::<TiltOptions>(&json).unwrap(), options(0.5));
}

/// A floor at z = 0 and, from x = 10 on, a block 60 high: a wall facing −X.
fn step_wall() -> Surface {
    let mut tris = Vec::new();
    for b in [block(Vec3::new(-40.0, -40.0, -10.0), Vec3::new(40.0, 40.0, 0.0)), block(Vec3::new(10.0, -40.0, 0.0), Vec3::new(40.0, 40.0, 60.0))] {
        tris.extend_from_slice(b.triangles());
    }
    Surface::new(tris).unwrap()
}

/// A 6 mm cutter 20 mm out of a 30 mm holder.
fn holder_tool(kind: ToolKind) -> Tool {
    let mut t = Tool::new(1, kind, 6.0, 10.0);
    if kind == ToolKind::BullNose {
        t.corner_radius = Some(1.0);
    }
    t.stickout = Some(20.0);
    t.holder = vec![HolderSection { diameter: 30.0, length: 40.0 }];
    t
}

/// Down beside the wall, along it on the floor and up again, as a 3-axis finish writes it.
fn beside_the_wall(x: f64) -> Toolpath5 {
    let p = |y: f64, z: f64| Pose::vertical(Vec3::new(x, y, z));
    Toolpath5 {
        moves: vec![
            Move5::Rapid { to: p(-20.0, 80.0) },
            Move5::Rapid { to: p(-20.0, 62.0) },
            Move5::Feed { to: p(-20.0, 0.0), feed: 100.0 },
            Move5::Feed { to: p(20.0, 0.0), feed: 500.0 },
            Move5::Rapid { to: p(20.0, 80.0) },
            Move5::Rapid { to: Pose::vertical(Vec3::new(-30.0, 20.0, 80.0)) },
        ],
    }
}

/// The path with its rapids (but the first move) made cutting moves, so that the checks of
/// cutting moves look at them too.
fn rapids_as_feeds(path: &Toolpath5) -> Toolpath5 {
    let moves = path
        .moves
        .iter()
        .enumerate()
        .map(|(i, m)| if let (Move5::Rapid { to }, true) = (*m, i > 0) { Move5::Feed { to, feed: 1.0 } } else { *m })
        .collect();
    Toolpath5 { moves }
}

#[test]
fn tilting_keeps_the_rapids_clear() {
    // Leant away from the wall along the floor, the tool must not swing back upright on the
    // rapid out (nor lean over on the rapid in) while its holder is still beside the wall.
    let s = step_wall();
    let tool = holder_tool(ToolKind::BallEndmill);
    let path = beside_the_wall(6.9);
    let opts = TiltOptions {
        max_tilt: deg(60.0),
        step: deg(1.0),
        about: TiltAbout::AwayFromPoint { point: Vec3::new(1000.0, 0.0, 0.0) },
        clearance: 1.0,
        max_axis_step: deg(5.0),
        sample_step: 0.5,
        tolerance: 0.01,
    };
    assert!(!holder_collisions(&s, &tool, &path, opts.clearance, 0.5).unwrap().is_empty());
    let out = tilt_away(&s, &tool, &path, &opts).unwrap();
    assert!(out.stats().max_tilt >= deg(20.0), "{}", out.stats().max_tilt.to_degrees());
    let all = rapids_as_feeds(&out);
    let h = holder_collisions(&s, &tool, &all, opts.clearance, 0.5).unwrap();
    assert!(h.is_empty(), "{h:?}");
    let g = gouges(&s, &tool.cutter().unwrap(), &all, opts.tolerance, 0.5).unwrap();
    assert!(g.is_empty(), "{g:?}");
    verify5(&out, opts.max_axis_step).unwrap();
}

#[test]
fn a_flat_end_is_not_leant_onto_the_top_of_the_wall() {
    // Leaning a flat or bull end towards the wall makes its endless side meet the wall, and
    // pushing it out along the axis then lifts it onto the top of the wall, 60 up: that lean
    // must not count as clear. Leant away, it stays on the floor.
    let s = step_wall();
    for kind in [ToolKind::FlatEndmill, ToolKind::BullNose] {
        let tool = holder_tool(kind);
        let c = tool.cutter().unwrap();
        let opts = TiltOptions { clearance: 0.5, ..options(deg(45.0)) };
        let out = tilt_away(&s, &tool, &beside_the_wall(6.9), &opts).unwrap();
        let mut on_floor = 0;
        for m in &out.moves {
            let Move5::Feed { to, .. } = *m else { continue };
            // The lowest point of the end: its corner's centre circle, less the corner.
            let lowest = to.tip.z - c.corner * (1.0 - to.axis.z) - c.flat_radius() * (1.0 - to.axis.z * to.axis.z).max(0.0).sqrt();
            assert!(lowest >= -1e-6, "{kind:?}: {to:?} cuts into the floor");
            assert!(to.axis.x <= 1e-12, "{kind:?}: it leans away from the wall: {to:?}");
            // Along the floor (past the plunge at y = −20) it cuts the floor.
            if to.tip.y > -20.0 + 1e-9 {
                assert!(lowest <= 1e-6, "{kind:?}: {to:?} is off the floor");
                on_floor += 1;
            }
        }
        assert!(on_floor >= 30, "{kind:?}: {on_floor}");
        assert!(gouges(&s, &c, &rapids_as_feeds(&out), opts.tolerance, 0.5).unwrap().is_empty());
        assert!(holder_collisions(&s, &tool, &rapids_as_feeds(&out), opts.clearance, 0.5).unwrap().is_empty());
    }
}

#[test]
fn a_tool_touching_a_wall_does_not_gouge_it() {
    // The ball's side exactly on the left wall of the pocket, a hair off it either way, and a
    // micron into it: within the tolerance, not a gouge the height of the wall.
    let s = pocket();
    let ball = Cutter::ball(3.0).unwrap();
    for x in [-7.0, -7.0 + 1e-13, -7.0 - 1e-13, -7.001] {
        let g = gouges(&s, &ball, &along_the_wall(x), 0.01, 0.5).unwrap();
        assert!(g.is_empty(), "{x}: {g:?}");
    }
    // Half a millimetre into the wall is a gouge.
    assert!(!gouges(&s, &ball, &along_the_wall(-7.5), 0.01, 0.5).unwrap().is_empty());
    // Into the floor, as deep as it cuts in.
    let low = path3(&[(Vec3::new(0.0, -20.0, 20.0), false), (Vec3::new(0.0, -20.0, -0.5), true), (Vec3::new(0.0, 20.0, -0.5), true)]);
    let g = gouges(&s, &ball, &low, 0.01, 0.5).unwrap();
    assert!(!g.is_empty() && g.iter().all(|h| (h.distance - 0.5).abs() <= 1e-9), "{g:?}");
    // A wall pass touching the wall is tilted clear of the holder, not refused as a gouge.
    let tool = short_tool();
    let out = tilt_away(&s, &tool, &along_the_wall(-7.0), &options(deg(40.0))).unwrap();
    assert!(holder_collisions(&s, &tool, &out, 0.5, 0.5).unwrap().is_empty());
}

/// The depth of a gouge is how deep the cutter is in the surface, square to it: on a 60° slope a
/// ball 0.05 mm too close to it is 0.05 mm in, though it would have to back out 0.1 mm along its
/// axis (what the check used to report). The ball along a move is measured exactly, and so is a
/// flat end, at its deepest sample.
#[test]
fn the_gouge_depth_is_measured_square_to_the_surface() {
    let th = deg(60.0);
    let s = slope(50.0, th);
    let (r, d) = (2.0, 0.05);
    let ball = Cutter::ball(r).unwrap();
    // The plane's unit normal; the ball's centre r − d from the plane, at y from -5 to 5.
    let n = Vec3::new(-th.sin(), 0.0, th.cos());
    let centre = |y: f64| Vec3::new(0.0, y, 0.0) + n * (r - d);
    let tip = |y: f64| centre(y) - Vec3::new(0.0, 0.0, r);
    let path = path3(&[(tip(-5.0) + Vec3::new(0.0, 0.0, 30.0), false), (tip(-5.0), true), (tip(5.0), true)]);
    let hits = gouges(&s, &ball, &path, 0.01, 0.5).unwrap();
    assert_eq!(hits.len(), 2, "{hits:?}");
    for h in &hits {
        close(h.distance, d, 1e-9);
    }
    close(depth_at(&s, &ball, &Pose::vertical(tip(0.0))).unwrap(), d, 1e-7);
    // A flat end 0.03 below a level floor is 0.03 in.
    let floor = plane(50.0, 0.0);
    let flat = Cutter::flat(3.0).unwrap();
    let low = path3(&[(Vec3::new(-5.0, 0.0, 10.0), false), (Vec3::new(-5.0, 0.0, -0.03), true), (Vec3::new(5.0, 0.0, -0.03), true)]);
    let hits = gouges(&floor, &flat, &low, 0.01, 0.5).unwrap();
    assert_eq!(hits.len(), 2, "{hits:?}");
    for h in &hits {
        close(h.distance, 0.03, 1e-7);
    }
    // Clear of the surface, or touching it, is no depth at all.
    assert_eq!(depth_at(&floor, &flat, &Pose::vertical(Vec3::new(0.0, 0.0, 0.5))).unwrap(), 0.0);
    assert!(depth_at(&floor, &flat, &Pose::vertical(Vec3::ZERO)).unwrap() < 1e-9);
    // Buried deeper than the radius: as far as it would have to back out.
    close(depth_at(&floor, &flat, &Pose::vertical(Vec3::new(0.0, 0.0, -5.0))).unwrap(), 5.0, 1e-9);
}

/// A gouge between two samples is found: a ball moving straight over the edge of a block,
/// clear at both ends of the move, is 0.03 mm into the edge half way, measured exactly; a flat
/// end over a rib narrower than its samples are apart is found by the samples between them.
#[test]
fn a_gouge_between_samples_is_found_and_measured() {
    let b = block(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 5.0));
    let ball = Cutter::ball(3.0).unwrap();
    // The ball's centre runs along y = 10 + 1, 2.8 above the top: 2.973 from the edge at
    // (·, 10, 5) at its nearest, so 0.027 in, but only along the block (|x| ≤ 10).
    let z = 5.0 + 2.8 - 3.0;
    let over = path3(&[(Vec3::new(-30.0, 11.0, 20.0), false), (Vec3::new(-30.0, 11.0, z), true), (Vec3::new(30.0, 11.0, z), true)]);
    let hits = gouges(&b, &ball, &over, 0.01, 2.0).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    let expect = 3.0 - 1.0f64.hypot(2.8);
    close(hits[0].distance, expect, 1e-9);
    assert!(hits[0].pose.tip.x.abs() <= 10.0 + 1e-9, "{:?}", hits[0].pose);
    // A rib 0.2 wide and 1 high on a floor, under a flat end 0.02 too low over it, crossed by a
    // move 10 long sampled every 4: it is found between the samples, as deep as it is.
    let mut tris = plane(50.0, 0.0).triangles().to_vec();
    tris.extend_from_slice(block(Vec3::new(4.9, -20.0, 0.0), Vec3::new(5.1, 20.0, 1.0)).triangles());
    let ribbed = Surface::new(tris).unwrap();
    let flat = Cutter::flat(1.0).unwrap();
    let across = path3(&[(Vec3::new(0.0, 0.0, 10.0), false), (Vec3::new(0.0, 0.0, 0.98), true), (Vec3::new(10.0, 0.0, 0.98), true)]);
    let hits = gouges(&ribbed, &flat, &across, 0.005, 4.0).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    close(hits[0].distance, 0.02, 1e-6);
}

/// The inside of a round hole of radius `r` about the Z axis from `z0` to `z1`, `n` facets round.
fn hole(r: f64, z0: f64, z1: f64, n: usize) -> Surface {
    let at = |j: usize, z: f64| {
        let th = TAU * (j % n) as f64 / n as f64;
        Vec3::new(r * th.cos(), r * th.sin(), z)
    };
    let mut tris = Vec::new();
    for j in 0..n {
        let (a, b, c, d) = (at(j, z0), at(j + 1, z0), at(j + 1, z1), at(j, z1));
        tris.push([a, c, b]);
        tris.push([a, d, c]);
    }
    Surface::new(tris).unwrap()
}

/// A shank no wider than the cutter, above the flutes, cannot hit a wall the cutter has just
/// cut: round a hole the cutter runs on the true wall, which is a hair outside the facets, and
/// the shank of its own diameter is that hair into them too. Within the gouge check's tolerance
/// that is no collision; a wider shank still is one.
#[test]
fn a_shank_as_wide_as_the_cutter_does_not_hit_the_hole_it_cuts() {
    let s = hole(3.0, -15.0, 0.0, 128);
    let mut tool = Tool::new(4, ToolKind::FlatEndmill, 4.0, 12.0);
    tool.shank_diameter = Some(4.0);
    tool.stickout = Some(20.0);
    tool.holder = vec![HolderSection { diameter: 34.0, length: 22.0 }];
    // Round the hole at radius 1, the tip 13 down: the shank is in the hole from 1 down up.
    let round: Vec<(Vec3, bool)> = std::iter::once((Vec3::new(1.0, 0.0, 10.0), false))
        .chain(
            (0..=64).map(|k| (Vec3::new(1.0, 0.0, -13.0) + Vec3::new((TAU * k as f64 / 64.0).cos() - 1.0, (TAU * k as f64 / 64.0).sin(), 0.0), true)),
        )
        .collect();
    let path = path3(&round);
    let sag = 3.0 * (1.0 - (PI / 128.0).cos());
    assert!(!holder_collisions(&s, &tool, &path, 0.0, 0.5).unwrap().is_empty(), "the facets are {sag} inside the true wall");
    let within = holder_collisions_within(&s, &tool, &path, 0.0, 0.005, 0.5).unwrap();
    assert!(within.is_empty(), "{within:?}");
    assert!(gouges(&s, &tool.cutter().unwrap(), &path, 0.005, 0.5).unwrap().is_empty());
    // A 5 mm shank above a 4 mm cutter is into the wall by half a millimetre: a collision, once
    // per move whatever the samples.
    tool.shank_diameter = Some(5.0);
    let hits = holder_collisions_within(&s, &tool, &path, 0.0, 0.005, 0.1).unwrap();
    assert!(!hits.is_empty() && hits.iter().all(|h| h.envelope == Some(0)) && hits.len() <= path.moves.len(), "{} hits", hits.len());
    assert!(holder_collisions_within(&s, &tool, &path, 0.0, f64::NAN, 0.5).is_err());
}

/// The deepest sample by the quick test is not always the deepest gouge: beside a tall wall
/// the cutter would have to back out the wall's height along its axis, though it is only a
/// hair into it. A flat end 0.03 mm into a plate at the start of a move and 0.012 mm into a
/// wall at its end is 0.03 mm deep there (it used to be reported as 0.012, the wall's depth).
#[test]
fn the_deepest_gouge_of_a_move_is_measured_whatever_the_quick_test_says() {
    let mut tris = block(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 0.03)).triangles().to_vec();
    tris.extend_from_slice(block(Vec3::new(12.0 - 0.012, -5.0, -5.0), Vec3::new(20.0, 5.0, 5.0)).triangles());
    let s = Surface::new(tris).unwrap();
    // A flat end of radius 2, its tip at Z 0 from X 0 to 10.
    let flat = Cutter::flat(2.0).unwrap();
    let path = path3(&[(Vec3::new(0.0, 0.0, 10.0), false), (Vec3::new(0.0, 0.0, 0.0), true), (Vec3::new(10.0, 0.0, 0.0), true)]);
    let hits = gouges(&s, &flat, &path, 0.01, 0.5).unwrap();
    let along = hits.iter().find(|h| h.index == 2).map(|h| h.distance);
    assert!(along.is_some_and(|d| (d - 0.03).abs() < 1e-6), "{hits:?}");
}

/// The end of a taper turned on a 4th axis, as the part's own tessellation gives it: the end
/// face in the plane X 30 (some of its corners a rounding off it) with an edge along the radial
/// line at 45°, and the taper's last facets meeting it at the rim.
fn taper_end() -> Surface {
    let v = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let (rim, half) = (v(30.0, 4.242640687119286, 4.242640687119286), v(30.0, 2.1213203435596437, 2.1213203435596437));
    let (f1, f2) = (v(30.000000000000004, 3.283370607972145, 3.0089100015804924), v(30.000000000000004, 3.008910001580494, 3.283370607972145));
    let (f3, f4) = (v(30.000000000000004, 4.0174348163531635, 4.456480393354539), v(30.000000000000004, 4.456480393354539, 4.0174348163531635));
    let (t1, t2) = (v(27.142857142857146, 4.765420117686176, 4.5248695640139305), v(27.142857142857146, 4.64670170494017, 4.646701704940169));
    let t3 = v(27.14285714285714, 4.524869564013933, 4.765420117686174);
    Surface::new(vec![[rim, half, f1], [rim, f2, half], [rim, f3, f2], [rim, f1, f4], [f4, t1, rim], [rim, t1, t2], [rim, t2, t3], [rim, t3, f3]])
        .unwrap()
}

/// A ball resting on the end of the taper, turned with A alone so its axis runs along the end
/// face's edge, is not in the part: the face, met edge-on by the line down the axis, is no
/// surface above the tip (a 0.7658 mm gouge was reported there).
#[test]
fn a_ball_whose_axis_runs_along_a_face_edge_on_does_not_gouge_it() {
    let s = taper_end();
    let ball = Cutter::ball(2.0).unwrap();
    let to = Pose::new(Vec3::new(30.0, 4.270647634931788, 4.27064763493179), Vec3::new(0.0, 0.7071067811865475, 0.7071067811865477));
    let from = Pose::new(Vec3::new(30.0, 4.157390151470782, 4.38097511465872), Vec3::new(0.0, 0.6883545756937539, 0.7253743710122877));
    assert_eq!(depth_at(&s, &ball, &to).unwrap(), 0.0);
    let path = Toolpath5 { moves: vec![Move5::Feed { to: from, feed: 1000.0 }, Move5::Feed { to, feed: 1000.0 }] };
    let hits = gouges(&s, &ball, &path, 0.0075, 1.0).unwrap();
    assert!(hits.is_empty(), "{hits:?}");
    // The line down the axis meets the part where it really does: the rim, just under the tip.
    let (t, _) = s.raycast(to.up(50.0), -to.axis).unwrap();
    assert!((50.0 - t + 0.0396).abs() < 1e-3, "met {} above the tip", 50.0 - t);
}

/// Whether a cutter is in the surface looks only at what reaches up to it: a face mill passing
/// over a finely tessellated hole tests no triangle of the hole below it, and finds the same
/// depths as the drop down the whole column where it is in the surface.
#[test]
fn a_cutter_above_the_surface_looks_at_nothing_below_it() {
    use crate::dropcutter::DROP_TRIANGLES;
    let s = hole(8.0, -30.0, 0.0, 4000);
    let mill = Cutter::flat(16.0).unwrap();
    let test = GougeTest::new(&mill, 0.01).unwrap();
    let count = |f: &dyn Fn()| {
        DROP_TRIANGLES.with(|n| n.set(0));
        f();
        DROP_TRIANGLES.with(|n| n.get())
    };
    let above = count(&|| {
        for k in 0..50 {
            let pose = Pose::vertical(Vec3::new(-25.0 + k as f64, 0.0, 5.0));
            assert_eq!(test.depth(&s, &pose).unwrap(), None);
            assert_eq!(depth_at(&s, &mill, &pose).unwrap(), 0.0);
        }
    });
    assert!(above <= 50, "{above} triangles tested above the hole");
    // Down in the hole the mill is in its wall, as deep as the whole column says.
    let pose = Pose::vertical(Vec3::new(0.0, 0.0, -10.0));
    let full = crate::dropcutter::drop_along(&s, &mill, pose.tip, pose.axis).unwrap();
    assert!(full.is_some_and(|d| d > 0.0));
    assert!(test.depth(&s, &pose).unwrap().is_some());
    assert!(depth_at(&s, &mill, &pose).unwrap() > 0.0);
}

/// A tool's envelopes stack up from its flutes: the shank to the holder face, then each holder
/// section. A tool saved before these fields existed still loads, with no envelope; values out
/// of range are refused.
#[test]
fn tool_envelopes_stack_up_from_the_flutes() {
    let mut tool = Tool::new(1, ToolKind::FlatEndmill, 6.0, 10.0);
    assert!(tool.envelopes().is_empty(), "no stickout past the flutes and no holder: nothing to check");
    tool.shank_diameter = Some(5.0);
    tool.stickout = Some(25.0);
    tool.holder = vec![HolderSection { diameter: 20.0, length: 15.0 }, HolderSection { diameter: 40.0, length: 30.0 }];
    tool.validate().unwrap();
    assert_eq!(
        tool.envelopes(),
        vec![
            Envelope { bottom: 10.0, top: 25.0, radius: 2.5 },
            Envelope { bottom: 25.0, top: 40.0, radius: 10.0 },
            Envelope { bottom: 40.0, top: 70.0, radius: 20.0 },
        ]
    );
    let json = serde_json::to_string(&tool).unwrap();
    assert_eq!(serde_json::from_str::<Tool>(&json).unwrap(), tool);
    let old: Tool = serde_json::from_str(r#"{"number":1,"kind":"flat_endmill","diameter":6.0,"flute_length":10.0}"#).unwrap();
    assert_eq!((old.shank_diameter, old.stickout, old.holder.len()), (None, None, 0));
    let refused = |f: &dyn Fn(&mut Tool)| {
        let mut t = tool.clone();
        f(&mut t);
        t.validate().is_err()
    };
    assert!(refused(&|t| t.shank_diameter = Some(0.0)));
    assert!(refused(&|t| t.shank_diameter = Some(f64::NAN)));
    assert!(refused(&|t| t.stickout = Some(5.0)), "shorter than the flutes");
    assert!(refused(&|t| t.stickout = Some(f64::INFINITY)));
    assert!(refused(&|t| t.flute_length = 2.0e4));
    assert!(refused(&|t| t.holder = vec![HolderSection { diameter: -1.0, length: 10.0 }]));
    assert!(refused(&|t| t.holder = vec![HolderSection { diameter: 10.0, length: f64::NAN }]));
    assert!(refused(&|t| t.holder = vec![HolderSection { diameter: 10.0, length: 1.0 }; 65]));
}
