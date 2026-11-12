//! Tests of 4th-axis G-code: writing continuous and indexed operations, inverse-time feeds,
//! turns of A clear of the work, rewinds, the posts and the layout switches of a post. Each
//! written cut is put back through the machine's forward kinematics and held against the
//! planned toolpath, and whole programs are read back with [`crate::readback5`].

use std::f64::consts::TAU;

use solidcore_record::{Loop2, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::foundation_tests::*;
use crate::gcode::words;
use crate::gcode5::*;
use crate::indexed::{IndexedOperation, frame_for_index};
use crate::kinematics::*;
use crate::readback5::{Readback5, readback_program, readback5};
use crate::*;

fn deg(d: f64) -> f64 {
    d.to_radians()
}

/// `+Z` at zero, `+Y` at 90°: what `A = θ` on a 4th axis brings up to the tool.
fn radial(theta: f64) -> Vec3 {
    Vec3::new(0.0, theta.sin(), theta.cos())
}

fn tool() -> Tool {
    Tool::new(3, ToolKind::BallEndmill, 6.0, 20.0)
}

fn op5(name: &str, path: Toolpath5) -> Operation5 {
    Operation5 { name: name.into(), tool: tool(), spindle_rpm: 12000.0, coolant: true, path, plunge_feed: None }
}

fn program5(path: Toolpath5) -> Program5 {
    Program5 { name: "part".into(), operations: vec![op5("op", path)], work_offset: None }
}

/// A post that turns A wherever it is told to after a retract to the machine's top (`G53 G0
/// Z0`), not at a clearance height of its own.
fn retracting() -> Post5 {
    Post5 { safe_rotate: vec!["G53 G0 Z0".into()], clearance_retract: false, ..Post5::linuxcnc_4axis() }
}

/// A wrapped helix: three turns round the X axis at radius 20, 30 mm along X.
fn helix() -> Toolpath5 {
    let n = 360;
    let pose = |k: usize| {
        let th = 3.0 * TAU * k as f64 / n as f64;
        Pose::new(Vec3::new(30.0 * k as f64 / n as f64, 0.0, 0.0) + radial(th) * 20.0, radial(th))
    };
    let mut moves = vec![Move5::Rapid { to: Pose::new(pose(0).up(10.0), radial(0.0)) }, Move5::Feed { to: pose(0), feed: 200.0 }];
    moves.extend((1..=n).map(|k| Move5::Feed { to: pose(k), feed: 300.0 }));
    moves.push(Move5::Feed { to: Pose::new(pose(n).up(10.0), radial(3.0 * TAU)), feed: 1000.0 });
    Toolpath5 { moves }
}

/// Radial drops round the X axis every 30°: down from radius 30 to 15, along X, back up,
/// then a rapid turn to the next angle.
fn radial_drops() -> Toolpath5 {
    let mut moves = Vec::new();
    for k in 0..12 {
        let th = deg(30.0 * k as f64);
        let at = |x: f64, r: f64| Pose::new(Vec3::new(x, 0.0, 0.0) + radial(th) * r, radial(th));
        moves.push(Move5::Rapid { to: at(0.0, 30.0) });
        moves.push(Move5::Feed { to: at(0.0, 15.0), feed: 150.0 });
        moves.push(Move5::Feed { to: at(25.0, 15.0), feed: 400.0 });
        moves.push(Move5::Feed { to: at(25.0, 30.0), feed: 1000.0 });
    }
    Toolpath5 { moves }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
    let v = |x: f64, y: f64| Vec2::new(x, y);
    Region2 { outer: Loop2::polygon(&[v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)]), holes: vec![] }
}

fn contour_path() -> Toolpath {
    let params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let cut =
        Cut { heights: Heights { clearance: 10.0, retract: 2.0, top: 0.0, bottom: -2.0 }, feeds: Feeds { cut: 600.0, plunge: 200.0 }, stepdown: 1.0 };
    let path = contour(&[rect(0.0, 0.0, 20.0, 10.0)], &Tool::new(3, ToolKind::FlatEndmill, 6.0, 20.0), &params, &cut).unwrap();
    assert!(path.moves.iter().any(|m| matches!(m, Move::Arc { .. })));
    path
}

fn lines_with<'a>(text: &'a str, start: &str) -> Vec<&'a str> {
    text.lines().filter(|l| l.starts_with(start)).collect()
}

/// One `G1` cut as written, with every axis known before and after it.
#[derive(Debug)]
struct WrittenCut {
    from: Joints,
    to: Joints,
    /// Whether it was written in inverse time (`G93`).
    inverse: bool,
    /// Its `F` word as it stands.
    f: f64,
    /// The minutes its `F` gives it.
    minutes: f64,
}

/// The `G1` cuts of a 4th-axis program in millimetres and radians: `G0`/`G1` with `X Y Z A F`,
/// `G20`/`G21`, `G93`/`G94`; a `G28`, `G30` or `G53` block makes the axes it names unknown.
/// Blocks before every axis is known, arcs and dwells are passed over (an arc still moves the
/// tool to its end).
fn written_cuts(text: &str) -> Vec<WrittenCut> {
    let (mut units, mut mode, mut inverse, mut feed) = (1.0, None::<i64>, false, None::<f64>);
    let mut at: [Option<f64>; 4] = [None; 4];
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let ws = words(line, i + 1).unwrap();
        let word = |l: char| ws.iter().find(|(c, _)| *c == l).map(|(_, v)| *v);
        let (mut home, mut dwell) = (false, false);
        for code in ws.iter().filter(|(c, _)| *c == 'G').map(|(_, v)| (v * 10.0).round() as i64) {
            match code {
                0 | 10 | 20 | 30 => mode = Some(code / 10),
                40 => dwell = true,
                200 => units = 25.4,
                210 => units = 1.0,
                280 | 300 | 530 => home = true,
                930 => (inverse, feed) = (true, None),
                940 => (inverse, feed) = (false, None),
                _ => {}
            }
        }
        if dwell {
            continue;
        }
        if let Some(f) = word('F') {
            feed = Some(f);
        }
        let new = [word('X').map(|v| v * units), word('Y').map(|v| v * units), word('Z').map(|v| v * units), word('A').map(f64::to_radians)];
        if home {
            for (slot, n) in at.iter_mut().zip(new) {
                if n.is_some() {
                    *slot = None;
                }
            }
            continue;
        }
        let before = at;
        for (slot, n) in at.iter_mut().zip(new) {
            if n.is_some() {
                *slot = n;
            }
        }
        let f = if inverse { word('F') } else { feed };
        if new.iter().all(Option::is_none) || mode != Some(1) {
            continue;
        }
        if let ([Some(x0), Some(y0), Some(z0), Some(a0)], [Some(x1), Some(y1), Some(z1), Some(a1)], Some(f)) = (before, at, f) {
            let from = Joints { linear: Vec3::new(x0, y0, z0), rotary: vec![a0] };
            let to = Joints { linear: Vec3::new(x1, y1, z1), rotary: vec![a1] };
            let length = from.linear.dist(to.linear);
            let minutes = if inverse {
                1.0 / f
            } else if length > 1e-9 {
                length / (f * units)
            } else {
                (a1 - a0).abs().to_degrees() / f
            };
            out.push(WrittenCut { from, to, inverse, f, minutes });
        }
    }
    out
}

/// The farthest the tool tip comes from the planned cutting moves of `planned`, at the end and
/// the middle of each written cut, the machine's axes moving evenly through it.
fn largest_deviation(cuts: &[WrittenCut], planned: &[&Toolpath5], machine: &Machine) -> f64 {
    let mut segments = Vec::new();
    for path in planned {
        let mut last: Option<Vec3> = None;
        for m in &path.moves {
            match m {
                Move5::Rapid { to } => last = Some(to.tip),
                Move5::Feed { to, .. } => {
                    if let Some(a) = last {
                        segments.push((a, to.tip));
                    }
                    last = Some(to.tip);
                }
                Move5::Dwell { .. } => {}
            }
        }
    }
    let mut worst = 0.0f64;
    for c in cuts {
        for t in [0.5, 1.0] {
            let tip = forward(machine, &c.from.lerp(&c.to, t)).tip;
            let d = segments.iter().map(|(a, b)| tip.dist_to_segment(*a, *b)).fold(f64::INFINITY, f64::min);
            worst = worst.max(d);
        }
    }
    worst
}

/// Checks that every cut of `text` lies on `program`'s toolpaths within the post's tolerance
/// (and the rounding of the words), and that the written minutes are the planned ones; then
/// that the read-back finds the same, both ways round and with the tool axis.
fn assert_on_path(text: &str, program: &Program5, post: &Post5, machine: &Machine) {
    let cuts = written_cuts(text);
    assert!(!cuts.is_empty(), "{text}");
    let paths: Vec<&Toolpath5> = program.operations.iter().map(|o| &o.path).collect();
    let worst = largest_deviation(&cuts, &paths, machine);
    assert!(worst <= post.tolerance + 1e-3, "{}: a cut {worst} mm off the toolpath\n{text}", post.base.name);
    let written: f64 = cuts.iter().map(|c| c.minutes).sum();
    let planned: f64 = paths.iter().map(|p| p.stats().cut_minutes).sum();
    close(written, planned, 2e-3 * planned);
    let rb = readback5(text, program, post, machine).unwrap();
    assert!(rb.within(), "{}: {rb:?}\n{text}", post.base.name);
    assert!(rb.max_tip_deviation <= worst + rb.allowed_tip, "{rb:?}");
    close(rb.cut_minutes, written, 1e-6 * written);
    close(rb.planned_cut_minutes, planned, 1e-9 * planned);
}

/// Checks that `text`, read back through `machine`, is within `post`'s precision and
/// tolerances of `program`'s toolpaths, and gives the read-back.
fn assert_reads_back(text: &str, program: &MachineProgram, post: &Post5, machine: &Machine) -> Readback5 {
    let rb = readback_program(text, program, post, machine).unwrap();
    assert!(rb.within(), "{}: {rb:?}\n{text}", post.base.name);
    rb
}

#[test]
fn three_axis_programs_match_the_3axis_emitter() {
    let post = Post5 { base: Post::rs274(), ..Post5::linuxcnc_4axis() };
    let machine = Machine::three_axis();
    let mut path = contour_path();
    path.moves.push(Move::Dwell { seconds: 0.5 });
    let old = emit(
        &Program {
            name: "part".into(),
            operations: vec![Operation { name: "c".into(), tool: tool(), spindle_rpm: 9000.0, coolant: true, path: path.clone() }],
            work_offset: Some(2),
        },
        &Post::rs274(),
    )
    .unwrap();
    // As an indexed operation (no rotary to turn) the arcs are kept: the very same text.
    let indexed = IndexedOperation { name: "c".into(), tool: tool(), spindle_rpm: 9000.0, coolant: true, rotary: vec![], path: path.clone() };
    let program = MachineProgram { name: "part".into(), operations: vec![MachineOperation::Indexed(indexed)], work_offset: Some(2) };
    let text = emit_program(&program, &post, &machine).unwrap();
    assert_eq!(text, old);
    // Read back, arcs and the dwell included, it is on the toolpath and takes the planned time.
    let rb = assert_reads_back(&text, &program, &post, &machine);
    close(rb.cut_minutes, rb.planned_cut_minutes, 1e-4 * rb.planned_cut_minutes);
    close(rb.cut_length, rb.planned_cut_length, 1e-4 * rb.planned_cut_length);
    // As a continuous one the arcs become lines; the lines-only toolpath gives the same text too.
    let lines_only = Toolpath { moves: path.moves.iter().copied().filter(|m| !matches!(m, Move::Arc { .. })).collect() };
    let old = emit(
        &Program {
            name: "p".into(),
            operations: vec![Operation { name: "l".into(), tool: tool(), spindle_rpm: 9000.0, coolant: false, path: lines_only.clone() }],
            work_offset: None,
        },
        &Post::rs274(),
    )
    .unwrap();
    let p5 = Program5 {
        name: "p".into(),
        operations: vec![Operation5 {
            name: "l".into(),
            tool: tool(),
            spindle_rpm: 9000.0,
            coolant: false,
            path: Toolpath5::from_3axis(&lines_only, 0.001).unwrap(),
            plunge_feed: None,
        }],
        work_offset: None,
    };
    let new = emit5(&p5, &post, &machine).unwrap();
    assert_eq!(new, old);
    // The 4th-axis reader finds the blocks the 3-axis reader finds.
    assert_eq!(parse5(&new, &[]).unwrap().len(), parse(&old).unwrap().len());
    assert!(readback5(&new, &p5, &post, &machine).unwrap().within());
}

#[test]
fn a_wrapped_helix_is_written_in_inverse_time() {
    let machine = Machine::four_axis_a();
    let post = Post5::linuxcnc_4axis();
    let program = program5(helix());
    let text = emit5(&program, &post, &machine).unwrap();
    assert!(text.starts_with("(part)\nG17 G21 G40 G49 G54 G80 G90 G91.1 G94\n"), "{text}");
    assert!(text.contains("M5\nT3 M6\nG43 H3\nS12000 M3\nM8\n"), "{text}");
    assert!(text.contains("G93 G1"));
    assert!(text.contains("G94 G1"));
    // The last cut turns nothing, so G94 is back already.
    assert!(text.ends_with("M9\nM5\nM30\n") && !text.ends_with("G94\nM9\nM5\nM30\n"));
    // Three whole turns, never wrapped.
    assert!(text.contains(" A1080"), "{text}");
    assert_on_path(&text, &program, &post, &machine);

    // Every inverse-time F is 1 / the minutes of its piece as the kinematics planned it.
    let pieces: Vec<f64> = solve(&machine, &program.operations[0].path, &post.solve_options())
        .unwrap()
        .windows(2)
        .filter_map(|w| match (&w[0], &w[1]) {
            (a, JointMove::Feed { to, minutes, .. }) => a.joints().filter(|j| j.rotary_change(to) > 1e-9).map(|_| *minutes),
            _ => None,
        })
        .collect();
    let written: Vec<f64> = written_cuts(&text).into_iter().filter(|c| c.inverse).map(|c| c.f).collect();
    assert_eq!(written.len(), pieces.len());
    for (f, minutes) in written.iter().zip(&pieces) {
        close(*f, 1.0 / minutes, 1e-4 / minutes);
    }
}

#[test]
fn rotaries_turn_only_at_the_clearance_height() {
    let machine = Machine::four_axis_a();
    for post in [Post5::linuxcnc_4axis(), Post5::grblhal_4axis()] {
        let program = program5(radial_drops());
        let text = emit5(&program, &post, &machine).unwrap();
        assert_on_path(&text, &program, &post, &machine);
        // Every block that turns A stands alone and is written with the tool at Z30 or above.
        let mut z: Option<f64> = None;
        let mut turns = 0;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("G0 Z") {
                z = rest.parse().ok();
            } else if line.starts_with("G0 X") || line.starts_with("G1") {
                z = line.split(' ').find_map(|w| w.strip_prefix('Z')).and_then(|v| v.parse().ok()).or(z);
            }
            if line.contains(" A") && line.starts_with("G0") {
                assert!(line.starts_with("G0 A"), "{line}");
                assert!(z.is_some_and(|z| z >= 30.0 - 1e-9), "{}: {line} at Z {z:?}", post.base.name);
                turns += 1;
            }
        }
        assert_eq!(turns, 12, "{}", post.base.name);
    }
    // The grblHAL post writes no M6 or tool length offset and comments with semicolons.
    let text = emit5(&program5(radial_drops()), &Post5::grblhal_4axis(), &machine).unwrap();
    assert!(!text.contains("M6") && !text.contains("G43") && text.starts_with("; part\nG90 G91.1 G94 G17 G21\n"));
}

#[test]
fn indexed_operations_keep_their_arcs() {
    let machine = Machine::four_axis_a();
    let post = Post5::linuxcnc_4axis();
    let path = contour_path();
    let at = |angle: f64, name: &str| {
        MachineOperation::Indexed(IndexedOperation {
            name: name.into(),
            tool: tool(),
            spindle_rpm: 10000.0,
            coolant: false,
            rotary: vec![angle],
            path: path.clone(),
        })
    };
    let program = MachineProgram {
        name: "sides".into(),
        operations: vec![at(deg(90.0), "side 1"), at(deg(180.0), "side 2"), MachineOperation::Continuous(op5("wrap", radial_drops()))],
        work_offset: None,
    };
    let text = emit_program(&program, &post, &machine).unwrap();
    assert!(!lines_with(&text, "G2 ").is_empty() || !lines_with(&text, "G3 ").is_empty());
    // At the start, where the tool's height is not known, up to the program's highest
    // clearance (the wrap's) before the first index; the next index at the operation's
    // clearance, where the last operation left the tool.
    assert!(text.contains("G43 H3\nS10000 M3\nG0 Z30\nG0 A90\n"), "{text}");
    assert!(text.contains("G0 Z10\n(side 2: T3 D6 ball end mill)\nG0 A180\n"), "{text}");
    // The written arcs are the 3-axis toolpath's own: in the index frame, the machine position
    // is the toolpath's coordinates.
    let frame = frame_for_index(&machine, &[deg(90.0)]).unwrap();
    let first_feed = path.moves.iter().find_map(|m| if let Move::Feed { to, .. } = m { Some(*to) } else { None }).unwrap();
    let j = inverse(&machine, &Pose::new(frame.to_world(first_feed), frame.z), None).unwrap();
    close3(j.linear, first_feed, 1e-9);
    let first_arc = text.lines().find(|l| l.starts_with("G2 ") || l.starts_with("G3 ")).unwrap();
    let planned_arc = path.moves.iter().find_map(|m| if let Move::Arc { to, .. } = m { Some(*to) } else { None }).unwrap();
    let word = |l: char| first_arc.split(' ').find_map(|w| w.strip_prefix(l)).and_then(|v| v.parse::<f64>().ok());
    close(word('X').unwrap(), planned_arc.x, 1e-4);
    close(word('Y').unwrap(), planned_arc.y, 1e-4);
    // Read back, every side and the wrap are on their toolpaths and take the planned time.
    let rb = assert_reads_back(&text, &program, &post, &machine);
    close(rb.cut_minutes, rb.planned_cut_minutes, 2e-3 * rb.planned_cut_minutes);
}

#[test]
fn machines_without_4th_axis_output_are_refused() {
    let program = program5(radial_drops());
    let post = Post5::linuxcnc_4axis();
    // Two rotaries: 5-axis output is not written yet.
    for machine in [Machine::table_table_ac(), Machine::head_table_bc(), Machine::head_head_ca()] {
        let e = emit5(&program, &post, &machine).unwrap_err();
        assert!(matches!(e, CamError::Unsupported(_)) && e.to_string().contains("at most one rotary axis"), "{e}");
    }
    // A 4th axis lettered B.
    let b = Machine {
        name: "b".into(),
        rotaries: vec![RotaryAxis::new(AxisLetter::B, Vec3::Y, Vec3::ZERO, Carrier::Table, None)],
        ..Machine::three_axis()
    };
    let e = emit5(&program, &post, &b).unwrap_err();
    assert!(matches!(e, CamError::Unsupported(_)) && e.to_string().contains("lettered A, not B"), "{e}");
    // The read-back refuses them the same way.
    let text = emit5(&program, &post, &Machine::four_axis_a()).unwrap();
    for machine in [Machine::table_table_ac(), b] {
        let e = readback5(&text, &program, &post, &machine).unwrap_err();
        assert!(matches!(e, CamError::Unsupported(_)), "{e}");
    }
}

#[test]
fn feeds_without_inverse_time_take_the_planned_time() {
    // G94: F is the linear axes' rate that takes the planned time.
    let machine = Machine::four_axis_a();
    let post = Post5 { inverse_time: false, ..Post5::linuxcnc_4axis() };
    let program = program5(helix());
    let text = emit5(&program, &post, &machine).unwrap();
    assert!(!text.contains("G93"));
    assert_on_path(&text, &program, &post, &machine);
}

#[test]
fn reversed_and_offset_rotaries_are_written_for_their_machine() {
    let post = Post5::linuxcnc_4axis();
    for machine in [Machine::four_axis_a_with(Vec3::ZERO, true, None), Machine::four_axis_a_at(Vec3::new(0.0, 2.0, -30.0))] {
        let program = program5(helix());
        let text = emit5(&program, &post, &machine).unwrap();
        assert_on_path(&text, &program, &post, &machine);
        if machine.rotaries[0].reversed {
            assert!(text.contains(" A-1080"));
        }
        // Put through another machine, it is off the toolpath.
        let other = Machine::four_axis_a_at(Vec3::new(0.0, 0.0, 5.0));
        assert!(largest_deviation(&written_cuts(&text), &[&program.operations[0].path], &other) > 1.0);
        // And the read-back says so: off by as far as it looks.
        let rb = readback5(&text, &program, &post, &other).unwrap();
        assert!(!rb.within() && rb.max_tip_deviation >= rb.reach, "{rb:?}");
    }
}

#[test]
fn posts_round_trip_and_validate() {
    for name in Post5::BUILTIN {
        let post = Post5::builtin(name).unwrap();
        post.validate().unwrap();
        let text = serde_json::to_string(&post).unwrap();
        assert_eq!(serde_json::from_str::<Post5>(&text).unwrap(), post);
    }
    assert!(Post5::builtin("nope").is_none());
    assert_eq!(Post5::builtin(" GRBLHAL "), Some(Post5::grblhal_4axis()));
    let post = Post5::linuxcnc_4axis();
    assert!(Post5 { angle_decimals: 9, ..post.clone() }.validate().is_err());
    assert!(Post5 { tolerance: f64::NAN, ..post.clone() }.validate().is_err());
    assert!(Post5 { clearance: Some(f64::INFINITY), ..post.clone() }.validate().is_err());
    assert!(Post5 { safe_rotate: vec!["G0 {nope}".into()], ..post.clone() }.validate().is_err());
    // A set clearance is used as it is.
    let text = emit5(&program5(radial_drops()), &Post5 { clearance: Some(55.0), ..post }, &Machine::four_axis_a()).unwrap();
    assert!(text.contains("G0 Z55\nG0 A30\n"));
}

#[test]
fn hostile_programs_are_refused() {
    let machine = Machine::four_axis_a();
    let post = Post5::linuxcnc_4axis();
    assert!(emit5(&Program5::default(), &post, &machine).is_err());
    let nan = Toolpath5 { moves: vec![Move5::Rapid { to: Pose::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Z) }] };
    assert!(emit5(&program5(nan), &post, &machine).is_err());
    let zero_feed =
        Toolpath5 { moves: vec![Move5::Rapid { to: Pose::vertical(Vec3::ZERO) }, Move5::Feed { to: Pose::vertical(Vec3::X), feed: 0.0 }] };
    assert!(emit5(&program5(zero_feed), &post, &machine).is_err());
    let mut bad_rpm = program5(helix());
    bad_rpm.operations[0].spindle_rpm = f64::NAN;
    assert!(emit5(&bad_rpm, &post, &machine).is_err());
    // The header of the grblHAL post sets no work offset.
    let mut offset = program5(helix());
    offset.work_offset = Some(2);
    assert!(emit5(&offset, &Post5::grblhal_4axis(), &machine).is_err());
    assert!(emit5(&offset, &post, &machine).unwrap().contains("G55"));
    // A tilted tool on a 3-axis machine.
    let tilted = Toolpath5 { moves: vec![Move5::Rapid { to: Pose::new(Vec3::ZERO, radial(deg(10.0))) }] };
    assert!(emit5(&program5(tilted), &post, &Machine::three_axis()).is_err());
    // Indexed operations with the wrong angles or hostile coordinates.
    let indexed = |rotary: Vec<f64>, path: Toolpath| MachineProgram {
        name: "i".into(),
        operations: vec![MachineOperation::Indexed(IndexedOperation {
            name: "i".into(),
            tool: tool(),
            spindle_rpm: 1000.0,
            coolant: false,
            rotary,
            path,
        })],
        work_offset: None,
    };
    assert!(emit_program(&indexed(vec![], contour_path()), &post, &machine).is_err());
    assert!(emit_program(&indexed(vec![f64::NAN], contour_path()), &post, &machine).is_err());
    let bad = Toolpath { moves: vec![Move::Rapid { to: Vec3::ZERO }, Move::Feed { to: Vec3::new(f64::INFINITY, 0.0, 0.0), feed: 100.0 }] };
    assert!(emit_program(&indexed(vec![0.0], bad), &post, &machine).is_err());
    let cut_first = Toolpath { moves: vec![Move::Feed { to: Vec3::X, feed: 100.0 }] };
    assert!(emit_program(&indexed(vec![0.0], cut_first), &post, &machine).is_err());

    // Reading.
    let a = [AxisLetter::A];
    assert!(matches!(parse5("G0 X0 Y0 Z0 B10", &a), Err(CamError::Parse(1, _))));
    assert!(matches!(parse5("G0 X0 Y0 Z0 A0\nG93 G1 X1 A10", &a), Err(CamError::Parse(2, _))));
    assert!(parse5("G0 X0 Y0 Z0 A0\nG93 G1 X1 A10 F5\nG1 X2 A20", &a).is_err());
    assert!(parse5("G91\nG0 X1", &a).is_err());
    assert!(parse5("G0 X0 Y0 Z0 A0\nG2 X1 Y1 I1 J0 A5 F100", &a).is_err());
    assert!(parse5("G0 X0 Y0 Z0 A0\nG93 G2 X1 Y1 I1 J0 F5", &a).is_err());
    assert!(parse5("G0 X0 Y0 Z0 A0\nG2 X1 Y1 F100", &a).is_err());
    assert!(parse5("G0 X0 Y0 Z0 A0\nG1 X1 F-5", &a).is_err());
    assert!(parse5("X1 Y2", &a).is_err());
    assert!(parse5("G0 X1..2", &a).is_err());
    // What this crate does not write yet is refused, not misread: tool centre point control.
    assert!(matches!(parse5("G43.4 H1\nG0 X0 Y0 Z0 A0", &a), Err(CamError::Parse(1, _))));
    // An arc in a vertical plane needs that plane's offsets.
    assert!(matches!(parse5("G0 X0 Y0 Z0 A0\nG18 G2 X1 Z1 J1 F100", &a), Err(CamError::Parse(2, _))));
    // G93 F applies to its own block; G53 makes the axes it names unknown; G20 scales the
    // linear axes, not the angles.
    let w = parse5("G20\nG0 X1 Y0 Z1 A0\nG93 G1 X2 A90 F4\nG53 G0 Z0\nG0 Z2\nG94 G1 X3 F10", &a).unwrap();
    assert_eq!(w.len(), 4);
    let JointWords::Feed { linear, rotary, f, inverse_time: true, .. } = &w[1] else { panic!("{w:?}") };
    close3(*linear, Vec3::new(50.8, 0.0, 25.4), 1e-12);
    close(rotary[0], deg(90.0), 1e-12);
    close(*f, 4.0, 1e-12);
    let JointWords::Rapid { linear, .. } = &w[2] else { panic!("{w:?}") };
    close(linear.z, 50.8, 1e-12);
    let JointWords::Feed { f, inverse_time: false, mm_per_unit, .. } = &w[3] else { panic!("{w:?}") };
    close(f * mm_per_unit, 254.0, 1e-12);
    // A dwell in seconds, and an arc about its centre from the start.
    let w = parse5("G0 X1 Y0 Z0 A0\nG4 P1.5\nG3 X-1 Y0 I-1 J0 F100", &a).unwrap();
    assert_eq!(w[1], JointWords::Dwell { line: 2, seconds: 1.5 });
    let JointWords::Arc { center, ccw: true, .. } = &w[2] else { panic!("{w:?}") };
    assert_eq!(*center, Vec2::ZERO);
}

fn indexed_op(name: &str, number: u32, rotary: Vec<f64>, moves: Vec<Move>) -> MachineOperation {
    MachineOperation::Indexed(IndexedOperation {
        name: name.into(),
        tool: Tool::new(number, ToolKind::FlatEndmill, 6.0, 20.0),
        spindle_rpm: 9000.0,
        coolant: false,
        rotary,
        path: Toolpath { moves },
    })
}

fn program_of(operations: Vec<MachineOperation>) -> MachineProgram {
    MachineProgram { name: "p".into(), operations, work_offset: None }
}

/// At every block that turns A: the Z there (`None` until written; `G53` sends it to the
/// machine's top) and the highest Z since the last cut.
fn rotary_turns(text: &str) -> Vec<(Option<f64>, f64)> {
    let mut z: Option<f64> = None;
    let mut peak = f64::NEG_INFINITY;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with("G53") {
            z = Some(f64::INFINITY);
        } else if let Some(v) = line.split(' ').find_map(|w| w.strip_prefix('Z')).and_then(|v| v.parse().ok()) {
            z = Some(v);
        }
        if line.contains("G1 ") || line.contains("G2 ") || line.contains("G3 ") {
            peak = f64::NEG_INFINITY;
        }
        peak = peak.max(z.unwrap_or(f64::NEG_INFINITY));
        if line.starts_with("G0 A") {
            out.push((z, peak));
        }
    }
    out
}

#[test]
fn a_rotary_never_turns_after_the_tool_has_come_down() {
    // A tall index (rapids at Z35) between two flat ones (rapids at Z10), the same tool: the
    // tool stays at Z35 while A turns back, rather than rapiding down to Z10 at A90 first.
    let machine = Machine::four_axis_a();
    let tall = indexed_op(
        "tall",
        3,
        vec![deg(90.0)],
        vec![
            Move::Rapid { to: Vec3::new(20.0, 0.0, 35.0) },
            Move::Feed { to: Vec3::new(20.0, 0.0, 28.0), feed: 200.0 },
            Move::Feed { to: Vec3::new(30.0, 0.0, 28.0), feed: 400.0 },
            Move::Rapid { to: Vec3::new(30.0, 0.0, 35.0) },
        ],
    );
    let flat = indexed_op(
        "flat",
        3,
        vec![0.0],
        vec![
            Move::Rapid { to: Vec3::new(0.0, 0.0, 10.0) },
            Move::Feed { to: Vec3::new(0.0, 0.0, 3.0), feed: 200.0 },
            Move::Feed { to: Vec3::new(10.0, 0.0, 3.0), feed: 400.0 },
            Move::Rapid { to: Vec3::new(10.0, 0.0, 10.0) },
        ],
    );
    let program = program_of(vec![flat.clone(), tall, flat]);
    let posts = [
        Post5::linuxcnc_4axis(),
        Post5::grblhal_4axis(),
        retracting(),
        Post5 { clearance_retract: true, ..retracting() },
        Post5 { clearance: Some(12.0), ..Post5::linuxcnc_4axis() },
    ];
    for post in posts {
        let text = emit_program(&program, &post, &machine).unwrap();
        assert_reads_back(&text, &program, &post, &machine);
        let turns = rotary_turns(&text);
        assert_eq!(turns.len(), 3, "{text}");
        for (z, peak) in &turns {
            // Never below where the tool was since its last cut, never at a height not known.
            assert!(z.is_some_and(|z| z >= *peak - 1e-9), "{}: a turn at Z {z:?} after Z {peak}\n{text}", post.base.name);
        }
        if post.safe_rotate.is_empty() {
            // At the start, up to the program's highest clearance; A0 again only from Z35.
            assert!(turns[0].0.is_some_and(|z| z >= 35.0 - 1e-9), "{text}");
            assert!(turns[2].0.is_some_and(|z| z >= 35.0 - 1e-9), "{text}");
        }
    }
}

#[test]
fn an_operation_rises_before_it_moves_across() {
    // A slot that ends at depth, then a hole elsewhere with the same tool or another: the
    // tool rises to Z15 before it moves across, never across at Z5 through the part.
    let machine = Machine::four_axis_a();
    let slot = vec![
        Move::Rapid { to: Vec3::new(0.0, 0.0, 15.0) },
        Move::Feed { to: Vec3::new(0.0, 0.0, 5.0), feed: 200.0 },
        Move::Feed { to: Vec3::new(20.0, 0.0, 5.0), feed: 300.0 },
    ];
    let hole = vec![
        Move::Rapid { to: Vec3::new(-10.0, 5.0, 15.0) },
        Move::Feed { to: Vec3::new(-10.0, 5.0, 5.0), feed: 200.0 },
        Move::Rapid { to: Vec3::new(-10.0, 5.0, 15.0) },
    ];
    for post in [Post5::linuxcnc_4axis(), Post5::grblhal_4axis()] {
        for second in [3, 4] {
            let program = program_of(vec![indexed_op("slot", 3, vec![0.0], slot.clone()), indexed_op("hole", second, vec![0.0], hole.clone())]);
            let text = emit_program(&program, &post, &machine).unwrap();
            let at = text.find("X-10 Y5").unwrap();
            assert!(text[..at].ends_with("G0 Z15\nG0 "), "{}: T{second}\n{text}", post.base.name);
            // The first operation's first rapid rises before it moves across as well.
            assert!(!text.contains("G0 X0 Y0\nG0 Z15"), "{text}");
        }
    }
    // The 3-axis emitter alike.
    let op = |name: &str, number: u32, moves: &[Move]| Operation {
        name: name.into(),
        tool: Tool::new(number, ToolKind::FlatEndmill, 6.0, 20.0),
        spindle_rpm: 9000.0,
        coolant: false,
        path: Toolpath { moves: moves.to_vec() },
    };
    for second in [3, 4] {
        let program = Program { name: "p".into(), operations: vec![op("slot", 3, &slot), op("hole", second, &hole)], work_offset: None };
        let text = emit(&program, &Post::rs274()).unwrap();
        let at = text.find("X-10 Y5").unwrap();
        assert!(text[..at].ends_with("G0 Z15\nG0 "), "T{second}\n{text}");
    }
}

/// A cut that turns A by 20° and ends there, the rapid after it turning nothing.
fn turning_cut(name: &str, number: u32) -> MachineOperation {
    let tool = Tool::new(number, ToolKind::BallEndmill, 6.0, 20.0);
    let path = Toolpath5 {
        moves: vec![
            Move5::Rapid { to: Pose::new(radial(0.0) * 30.0, radial(0.0)) },
            Move5::Feed { to: Pose::new(radial(0.0) * 20.0, radial(0.0)), feed: 200.0 },
            Move5::Feed { to: Pose::new(radial(deg(20.0)) * 20.0, radial(deg(20.0))), feed: 300.0 },
            Move5::Rapid { to: Pose::new(radial(deg(20.0)) * 30.0, radial(deg(20.0))) },
        ],
    };
    MachineOperation::Continuous(Operation5 { name: name.into(), tool, spindle_rpm: 12000.0, coolant: true, path, plunge_feed: None })
}

#[test]
fn inverse_time_ends_before_a_tool_change() {
    let machine = Machine::four_axis_a();
    for post in [Post5::linuxcnc_4axis(), Post5::grblhal_4axis(), retracting()] {
        let program = program_of(vec![turning_cut("a", 3), turning_cut("b", 4)]);
        let text = emit_program(&program, &post, &machine).unwrap();
        // G94 is in force from the end of the first operation's cuts: before the tool change,
        // spindle and coolant blocks of the next.
        assert_reads_back(&text, &program, &post, &machine);
        let change = text.find("(b:").or_else(|| text.find("; b:")).unwrap();
        let before = &text[..change];
        assert!(before.rfind("G94").unwrap() > before.rfind("G93").unwrap(), "{}\n{text}", post.base.name);
    }
}

#[test]
fn tool_changes_stop_the_spindle_and_coolant() {
    let machine = Machine::four_axis_a();
    let program = program_of(vec![turning_cut("a", 3), turning_cut("b", 4)]);
    // Coolant and spindle off before M6, both on again after.
    let text = emit_program(&program, &Post5::linuxcnc_4axis(), &machine).unwrap();
    assert!(text.contains("M9\nM5\nT4 M6\nG43 H4\nS12000 M3\nM8\n"), "{text}");
    // grblHAL: a manual change, the operator told which tool, the program paused.
    let text = emit_program(&program, &Post5::grblhal_4axis(), &machine).unwrap();
    assert!(text.contains("M9\nM5\n; change to tool 4, diameter 6\nM0\nM3 S12000\nM8\n"), "{text}");
    assert!(!text.contains("M6") && !text.contains("G43"));
}

#[test]
fn presets_write_their_units_and_incremental_arc_centres() {
    let machine = Machine::four_axis_a();
    for post in [Post5::linuxcnc_4axis(), Post5::grblhal_4axis()] {
        let text = emit5(&program5(helix()), &post, &machine).unwrap();
        let header = text.lines().nth(1).unwrap_or_default();
        assert!(header.contains("G91.1") && header.contains("G21"), "{header}");
        // In inches the header says so, and the program is still on the toolpath.
        let mut inch = post.clone();
        inch.base.units = Units::Inch;
        let program = program5(helix());
        let text = emit5(&program, &inch, &machine).unwrap();
        let header = text.lines().nth(1).unwrap_or_default();
        assert!(header.contains("G20") && !header.contains("G21"), "{header}");
        assert_on_path(&text, &program, &inch, &machine);
    }
    // The 3-axis reader takes G91.1 and refuses G90.1 (absolute arc centres), and so does the
    // 4th-axis one.
    assert!(matches!(parse("G90.1\nG0 X0 Y0 Z0"), Err(CamError::Parse(1, _))));
    assert_eq!(parse("G91.1 G0 X0 Y0 Z0\nG2 X2 Y0 I1 J0 F100").unwrap().len(), 2);
    let a = [AxisLetter::A];
    assert!(matches!(parse5("G90.1\nG0 X0 Y0 Z0 A0", &a), Err(CamError::Parse(1, _))));
    assert_eq!(parse5("G91.1\nG0 X0 Y0 Z0 A0\nG2 X2 Y0 I1 J0 F100", &a).unwrap().len(), 2);
}

#[test]
fn feeds_are_never_written_as_zero_or_absurd() {
    let machine = Machine::four_axis_a();
    let post = Post5::linuxcnc_4axis();
    let cut = |feed: f64| Toolpath5 {
        moves: vec![Move5::Rapid { to: Pose::vertical(Vec3::ZERO) }, Move5::Feed { to: Pose::vertical(Vec3::new(1.0, 1.0, 0.0)), feed }],
    };
    for feed in [1e-300, 1e-5, 4e-5, 2e6, 1e308] {
        assert!(emit5(&program5(cut(feed)), &post, &machine).is_err(), "{feed}");
    }
    let fine = Post5 { base: Post { feed_decimals: 4, ..post.base.clone() }, ..post.clone() };
    assert!(emit5(&program5(cut(1e-3)), &fine, &machine).is_ok());
    // At the post's one decimal a feed of 0.001 would be written as F0.
    assert!(emit5(&program5(cut(1e-3)), &post, &machine).is_err());
    let indexed = program_of(vec![indexed_op("i", 3, vec![0.0], vec![Move::Rapid { to: Vec3::ZERO }, Move::Feed { to: Vec3::X, feed: 1e-9 }])]);
    assert!(emit_program(&indexed, &post, &machine).is_err());
    let flat = Program {
        name: "p".into(),
        operations: vec![Operation {
            name: "o".into(),
            tool: tool(),
            spindle_rpm: 1000.0,
            coolant: false,
            path: Toolpath { moves: vec![Move::Rapid { to: Vec3::ZERO }, Move::Feed { to: Vec3::X, feed: 1e-5 }] },
        }],
        work_offset: None,
    };
    assert!(emit(&flat, &Post::rs274()).is_err());
    // A very long, slow cut that turns A: its inverse-time F is tiny but never F0.
    let slow = Toolpath5 {
        moves: vec![
            Move5::Rapid { to: Pose::new(radial(0.0) * 20.0, radial(0.0)) },
            Move5::Feed { to: Pose::new(Vec3::new(4.0e6, 0.0, 0.0) + radial(deg(1.0)) * 20.0, radial(deg(1.0))), feed: 1e-3 },
        ],
    };
    let text = emit5(&program5(slow), &fine, &machine).unwrap();
    assert!(text.contains("G93 G1") && !text.contains(" F0\n"), "{text}");
    assert!(written_cuts(&text).iter().any(|c| c.inverse && c.f > 0.0 && c.f < 1e-9), "{text}");
    // A tool with an absurd diameter is refused.
    let mut wide = program5(helix());
    wide.operations[0].tool = Tool::new(3, ToolKind::FlatEndmill, 1e300, 20.0);
    assert!(emit5(&wide, &post, &machine).is_err());
}

/// A post that writes, as data alone, the conventions of a hobby-mill controller's programs: a
/// tool list, capital comments from a few characters, a blank line and the operation's name
/// before each operation, `S… M3`, `G17 G90 G94` and the work offset at every operation, the
/// tool length offset on the first Z, modal motion codes, three decimals with a point on whole
/// numbers and whole feeds, a `G28 G91 Z0.` retract before every tool change, every turn of A
/// and the end, inverse time with four decimals, and rewinds.
fn dialect() -> Post5 {
    let retract = vec!["G28 G91 Z0.".to_string(), "G90".to_string()];
    let lines = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Post5 {
        base: Post {
            name: "dialect".into(),
            extension: "tap".into(),
            decimals: 3,
            feed_decimals: 0,
            trailing_decimal: true,
            header: lines(&["G90 G94 G91.1 G40 G49 G17", "{units}"]),
            tool_change: lines(&["T{tool} M6"]),
            tool_change_first: true,
            spindle_on: lines(&["S{rpm} M3"]),
            footer: lines(&["G28 G91 X0. Y0.", "G90", "M30"]),
            upper_case_comments: true,
            comment_chars: Some(" .,=_-".into()),
            tool_list: true,
            operation_name_only: true,
            blank_lines: true,
            operation_start: lines(&["G17 G90 G94", "{work_offset}"]),
            safe_retract: Some(SafeRetract { lines: retract.clone(), at_start: true, at_tool_change: true, at_end: true }),
            tool_length_on_first_z: true,
            modal_motion: true,
            ..Post::rs274()
        },
        safe_rotate: retract,
        safe_rotate_first: true,
        angle_decimals: 3,
        inverse_time_decimals: Some(4),
        rewind: Some(Rewind { distance: 25.0, retract_feed: 500.0, plunge_feed: 250.0 }),
        ..Post5::linuxcnc_4axis()
    }
}

/// Two 3-axis operations with two tools: a contour with a flat end mill, then a pocket with a
/// ball end mill, both with coolant.
fn dialect_job() -> Program {
    let flat = Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0);
    let ball = Tool::new(2, ToolKind::BallEndmill, 4.0, 20.0);
    let cut = |bottom: f64| Cut {
        heights: Heights { clearance: 15.0, retract: 5.0, top: 0.0, bottom },
        feeds: Feeds { cut: 1600.0, plunge: 50.0 },
        stepdown: 1.0,
    };
    let contour_params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let pocket_params = PocketParams { stepover: 1.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let outline = contour(&[rect(0.0, 0.0, 40.0, 20.0)], &flat, &contour_params, &cut(-2.0)).unwrap();
    let hollow = pocket(&[rect(10.0, 5.0, 30.0, 15.0)], &ball, &pocket_params, &cut(-1.25)).unwrap();
    Program {
        name: "part-1".into(),
        operations: vec![
            Operation { name: "Contour #1 (outer)".into(), tool: flat, spindle_rpm: 18000.0, coolant: true, path: outline },
            Operation { name: "Pocket1".into(), tool: ball, spindle_rpm: 24000.0, coolant: true, path: hollow },
        ],
        work_offset: None,
    }
}

/// A wrapped helix of `turns` turns round the X axis at radius 20, 10 mm along X a turn.
fn helix_of(turns: usize) -> Toolpath5 {
    let n = 120 * turns;
    let pose = |k: usize| {
        let th = TAU * k as f64 / 120.0;
        Pose::new(Vec3::new(10.0 * k as f64 / 120.0, 0.0, 0.0) + radial(th) * 20.0, radial(th))
    };
    let mut moves = vec![Move5::Rapid { to: Pose::new(pose(0).up(10.0), radial(0.0)) }, Move5::Feed { to: pose(0), feed: 200.0 }];
    moves.extend((1..=n).map(|k| Move5::Feed { to: pose(k), feed: 300.0 }));
    moves.push(Move5::Feed { to: Pose::new(pose(n).up(10.0), pose(n).axis), feed: 1000.0 });
    Toolpath5 { moves }
}

/// A 4th axis that turns at most a turn either way, its centreline on the work's X axis.
fn limited_a() -> Machine {
    Machine::four_axis_a_with(Vec3::ZERO, false, Some((-TAU, TAU)))
}

/// Checks that a motion code is written only when the mode changes (a template line or a tool
/// length offset line sets no mode the next block may leave out; a turn of A states its G0 and
/// the move after it may leave that out), that every number of a motion word has a decimal
/// point, and that every arc states both I and J.
fn assert_modal_with_points(text: &str) {
    let mut mode: Option<&str> = None;
    for line in text.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        let code = words.iter().copied().find(|w| ["G0", "G1", "G2", "G3"].contains(w));
        let motion = words.iter().any(|w| w.len() > 1 && "XYZIJAF".contains(&w[..1]) && w[1..].parse::<f64>().is_ok());
        if !motion || line.starts_with("G28") {
            mode = None;
            continue;
        }
        for w in &words {
            if w.len() > 1 && "XYZIJAF".contains(&w[..1]) && w[1..].parse::<f64>().is_ok() {
                assert!(w.contains('.'), "`{w}` without a decimal point in `{line}`");
            }
        }
        match code {
            Some(c) => {
                assert!(mode != Some(c) || words.iter().any(|w| w.starts_with('A')), "`{c}` written again in `{line}`");
                mode = Some(c);
            }
            None => assert!(mode.is_some() || line.starts_with("G43"), "no motion mode for `{line}`"),
        }
        if matches!(mode, Some("G2" | "G3")) && code.is_some() {
            assert!(line.contains(" I") && line.contains(" J"), "an arc without both I and J: `{line}`");
        }
    }
}

#[test]
fn a_dialect_post_writes_its_conventions_as_data() {
    let post = dialect();
    post.validate().unwrap();
    let program = dialect_job();
    let text = emit(&program, &post.base).unwrap();
    let start = "\
(PART-1)
(T1 D=6. CR=0. - ZMIN=-2. - FLAT END MILL)
(T2 D=4. CR=2. - ZMIN=-1.25 - BALL END MILL)
G90 G94 G91.1 G40 G49 G17
G21
G28 G91 Z0.
G90

(CONTOUR 1 OUTER)
T1 M6
S18000 M3
G17 G90 G94
G54
M8
G0 X0. Y-3.
G43 Z15. H1
";
    assert!(text.starts_with(start), "{text}");
    // The spindle stops and the tool rises before the next tool; coolant goes off for the change.
    let change = "\
G0 Z15.
M5
G28 G91 Z0.
G90

(POCKET1)
M9
T2 M6
S24000 M3
G17 G90 G94
G54
M8
G0 X13.5 Y8.5
G43 Z15. H2
";
    assert!(text.contains(change), "{text}");
    let end = "\
G0 Z15.

M9
M5
G28 G91 Z0.
G90
G28 G91 X0. Y0.
G90
M30
";
    assert!(text.ends_with(end), "{text}");
    assert_modal_with_points(&text);
    // Feeds after a cut in the same mode leave the code out.
    assert!(text.lines().any(|l| l == "Z-1."), "{text}");
    assert!(readback(&text, &program, &post.base).unwrap().within());

    // The multi-axis writer writes the very same program for a 3-axis machine.
    let indexed = MachineProgram {
        name: program.name.clone(),
        operations: program
            .operations
            .iter()
            .map(|o| {
                MachineOperation::Indexed(IndexedOperation {
                    name: o.name.clone(),
                    tool: o.tool.clone(),
                    spindle_rpm: o.spindle_rpm,
                    coolant: o.coolant,
                    rotary: vec![],
                    path: o.path.clone(),
                })
            })
            .collect(),
        work_offset: None,
    };
    let machine = Machine::three_axis();
    let text5 = emit_program(&indexed, &post, &machine).unwrap();
    assert_eq!(text5, text);
    assert_reads_back(&text5, &indexed, &post, &machine);

    // The work offset is written at every operation.
    let mut offset = program.clone();
    offset.work_offset = Some(2);
    let text = emit(&offset, &post.base).unwrap();
    assert_eq!(lines_with(&text, "G55").len(), 2);
    assert!(lines_with(&text, "G54").is_empty());
    // An arc leaves out the end coordinate that does not change.
    let half = Toolpath {
        moves: vec![
            Move::Rapid { to: Vec3::new(1.0, 0.0, 5.0) },
            Move::Feed { to: Vec3::new(1.0, 0.0, 0.0), feed: 100.0 },
            Move::Arc { to: Vec3::new(-1.0, 0.0, -0.5), center: Vec2::ZERO, ccw: true, feed: 100.0 },
        ],
    };
    let mut arc = program.clone();
    arc.operations.truncate(1);
    arc.operations[0].path = half;
    let text = emit(&arc, &post.base).unwrap();
    assert!(text.contains("\nG3 X-1. Z-0.5 I-1. J0.\n"), "{text}");
    assert!(readback(&text, &arc, &post.base).unwrap().within());
    // Lines after every operation's motion.
    let closing = Post { operation_end: vec!["G17".into()], ..post.base.clone() };
    let text = emit(&program, &closing).unwrap();
    assert!(text.contains("G0 Z15.\nG17\nM5\n") && text.contains("G0 Z15.\nG17\n\nM9\n"), "{text}");
}

#[test]
fn a_dialect_post_retracts_before_every_turn_of_a() {
    let post = dialect();
    let machine = Machine::four_axis_a();
    let side = |name: &str, a: f64| indexed_op(name, 3, vec![deg(a)], contour_path().moves);
    let program = MachineProgram {
        name: "wrap".into(),
        operations: vec![side("side 0", 0.0), side("side 90", 90.0), MachineOperation::Continuous(op5("helix", helix_of(1)))],
        work_offset: None,
    };
    let text = emit_program(&program, &post, &machine).unwrap();
    // No lowest Z where A turns.
    assert!(text.starts_with("(WRAP)\n(T3 D=6. CR=0. - FLAT END MILL)\n"), "{text}");
    // The first index turns A after the start's retract; the next one retracts before its
    // comment, then A turns alone and the tool comes back across (still G0, not written
    // again) and down with its offset.
    assert!(text.contains("\n(SIDE 0)\nT3 M6\nS9000 M3\nG17 G90 G94\nG54\nG0 A0.\nX0. Y-3.\nG43 Z10. H3\nZ1.\n"), "{text}");
    assert!(text.contains("\nG0 Z10.\nG28 G91 Z0.\nG90\n\n(SIDE 90)\nG17 G90 G94\nG54\nG0 A90.\nX0. Y-3.\nG43 Z10. H3\nZ1.\n"), "{text}");
    // The move after a turn of A does not repeat its G0.
    let mut last: Option<&str> = None;
    for line in text.lines() {
        assert!(!(line.starts_with("G0 ") && last.is_some_and(|l| l.starts_with("G0 A"))), "a repeated G0 after {last:?}: {line}\n{text}");
        last = Some(line);
    }
    assert_modal_with_points(&text);
    // Inverse time: F with at most four decimals on every block that turns A, G94 before the
    // next cut that does not.
    let mut inverse = false;
    let mut timed = 0;
    for line in text.lines() {
        if line.contains("G93") {
            inverse = true;
        }
        if line.contains("G94") {
            inverse = false;
        }
        if inverse && line.contains(" A") {
            let f = line.split(' ').find_map(|w| w.strip_prefix('F'));
            assert!(f.is_some_and(|f| f.split('.').nth(1).is_some_and(|d| d.len() <= 4)), "`F{f:?}` in `{line}`");
            timed += 1;
        }
        if let Some(a) = line.split(' ').find_map(|w| w.strip_prefix('A')) {
            assert!(a.split('.').nth(1).is_some_and(|d| d.len() <= 3), "`A{a}` in `{line}`");
        }
        if !inverse && line.starts_with('X') {
            assert!(!line.contains(" A"), "a turn of A per minute: `{line}`");
        }
    }
    assert!(timed >= 120, "{timed}");
    assert!(text.contains("G94 Z30. F1000.\n\nM9\nM5\nG28 G91 Z0.\nG90\n"), "{text}");
    // The helix is on its toolpath.
    let helix_text = &text[text.find("(HELIX)").unwrap()..];
    let cuts: Vec<WrittenCut> = written_cuts(helix_text).into_iter().filter(|c| c.inverse).collect();
    let MachineOperation::Continuous(helix) = &program.operations[2] else { panic!("the third operation is the helix") };
    assert!(largest_deviation(&cuts, &[&helix.path], &machine) <= post.tolerance + 1e-3);
    // The whole program reads back: the returns home between operations make the axes they
    // name unknown until the next move states them.
    assert_reads_back(&text, &program, &post, &machine);
}

/// The lines of the rewinds in `text`: from the draw out of the cut to the first cut in
/// inverse time after the way back in.
fn rewinds_in(text: &str) -> Vec<Vec<&str>> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with("G94 Z") && l.ends_with(" F500."))
        .map(|(i, _)| {
            let end = lines[i..].iter().position(|l| l.starts_with("G93")).map_or(lines.len(), |k| i + k + 1);
            lines[i..end].to_vec()
        })
        .collect()
}

/// The cuts of `text` that turn A, which a rewind's straight draw out and way back in do not.
fn turning_cuts(text: &str) -> Vec<WrittenCut> {
    written_cuts(text).into_iter().filter(|c| c.from.rotary_change(&c.to) > 1e-12).collect()
}

/// A groove once round a bar of radius 16 drawn 100.531 mm long, a hair more than a whole
/// turn (by 2·10⁻⁶ rad, less than the angle written can show): it starts at A0 and ends at
/// A360 within the limits, with no rewind and no turn before it.
#[test]
fn a_wrap_that_ends_at_a_whole_turn_needs_no_rewind() {
    let turn = 100.531 / 16.0;
    assert!(turn > TAU);
    let n = 180;
    let pose = |k: usize| {
        let th = turn * k as f64 / n as f64;
        Pose::new(Vec3::new(20.0, 0.0, 0.0) + radial(th) * 15.5, radial(th))
    };
    let mut moves = vec![Move5::Rapid { to: Pose::new(pose(0).up(5.0), radial(0.0)) }, Move5::Feed { to: pose(0), feed: 100.0 }];
    moves.extend((1..=n).map(|k| Move5::Feed { to: pose(k), feed: 300.0 }));
    // Out of the groove at rapid, still at its last angle.
    moves.push(Move5::Rapid { to: Pose::new(pose(n).up(5.0), pose(n).axis) });
    let program = program5(Toolpath5 { moves });
    let post = dialect();
    let text = emit5(&program, &post, &limited_a()).unwrap();
    assert!(rewinds_in(&text).is_empty(), "{text}");
    let angles: Vec<f64> = text.lines().filter_map(|l| l.split(' ').find_map(|w| w.strip_prefix('A'))).map(|a| a.parse().unwrap()).collect();
    assert_eq!(angles.first(), Some(&0.0), "{text}");
    assert_eq!(angles.iter().filter(|a| **a == 0.0).count(), 1, "A turns back to 0: {text}");
    assert_eq!(angles.last(), Some(&360.0), "{text}");
    assert!(angles.windows(2).all(|w| w[1] >= w[0]), "A only goes on round: {text}");
    assert_on_path(&text, &program, &post, &limited_a());
}

#[test]
fn a_long_helix_rewinds_within_the_limits_of_a() {
    let post = dialect();
    // The first plunge at the rewinds' plunge feed.
    let mut path = helix_of(5);
    if let Some(Move5::Feed { feed, .. }) = path.moves.get_mut(1) {
        *feed = 250.0;
    }
    let program = program5(path);
    // An A that turns one turn either way could not cut five turns in one go.
    assert!(emit5(&program, &Post5 { rewind: None, ..post.clone() }, &limited_a()).is_err());
    let text = emit5(&program, &post, &limited_a()).unwrap();
    let angles: Vec<f64> = text.lines().filter_map(|l| l.split(' ').find_map(|w| w.strip_prefix('A'))).map(|a| a.parse().unwrap()).collect();
    assert!(angles.iter().all(|a| a.abs() <= 360.0), "{text}");
    // Up to 360°, back to -360° (the same position), on to 360° again, back, on to 1800° - 1440°.
    assert_eq!(angles.last(), Some(&360.0));
    let rewinds = rewinds_in(&text);
    assert_eq!(rewinds.len(), 2, "{text}");
    // Fed 1 mm out of the cut, up at rapid, the turn after the safe retract, back at rapid to
    // 1 mm above the cut and fed the rest of the way in at the plunge feed.
    assert_eq!(
        rewinds[0],
        [
            "G94 Z21. F500.",
            "G0 Z30.",
            "G28 G91 Z0.",
            "G90",
            "G0 A-360.",
            "X10. Y0.",
            "G43 Z21. H3",
            "G1 Z20. F250.",
            "G93 X10.042 Z19.993 A-358.5 F571.2171"
        ]
    );
    assert!(rewinds[1].join("\n").starts_with("G94 Z21. F500.\nG0 Z30.\nG28 G91 Z0.\nG90\nG0 A-360.\nX30. Y0.\nG43 Z21. H3\nG1 Z20. F250."));
    // Every cut that turns A is on the helix; each rewind feeds 1 mm out and 1 mm back in.
    let planned = &program.operations[0].path;
    assert!(largest_deviation(&turning_cuts(&text), &[planned], &limited_a()) <= post.tolerance + 1e-3);
    let minutes: f64 = written_cuts(&text).iter().map(|c| c.minutes).sum();
    close(minutes, planned.stats().cut_minutes + 2.0 * (1.0 / 500.0 + 1.0 / 250.0), 2e-3 * minutes);
    // Read back, only the ends in the cut of a rewind's draw out and way back in are held
    // against the helix, and they count in the cut length and time as written. The first
    // plunge, at the rewinds' plunge feed, is still looked for in what is written.
    let rb = readback5(&text, &program, &post, &limited_a()).unwrap();
    assert!(rb.within(), "{rb:?}");
    close(rb.cut_length, rb.planned_cut_length + 2.0 * 2.0, 1e-3 * rb.planned_cut_length);
    close(rb.cut_minutes, minutes, 1e-6 * minutes);
    // The way back in takes the operation's plunge feed, else its tool's.
    let mut own = program.clone();
    own.operations[0].plunge_feed = Some(120.0);
    let text = emit5(&own, &post, &limited_a()).unwrap();
    assert!(!rewinds_in(&text).is_empty() && rewinds_in(&text).iter().all(|r| r.contains(&"G1 Z20. F120.")), "{text}");
    assert!(readback5(&text, &own, &post, &limited_a()).unwrap().within());
    let mut tool_feed = program.clone();
    tool_feed.operations[0].tool.plunge_feed = Some(90.0);
    let text = emit5(&tool_feed, &post, &limited_a()).unwrap();
    assert!(!rewinds_in(&text).is_empty() && rewinds_in(&text).iter().all(|r| r.contains(&"G1 Z20. F90.")), "{text}");
    assert!(readback5(&text, &tool_feed, &post, &limited_a()).unwrap().within());
    // A program read back against toolpaths it was not written from does not fit.
    let gone = Program5 { operations: vec![op5("op", helix_of(4))], ..program.clone() };
    assert!(!readback5(&text, &gone, &post, &limited_a()).unwrap().within());

    // Limits that a cut reaches part way through a piece: the piece is split there, each part
    // timed for its share.
    let tight = Machine::four_axis_a_with(Vec3::ZERO, false, Some((-TAU + 0.05, TAU - 0.05)));
    let text = emit5(&program, &post, &tight).unwrap();
    let limit = (TAU - 0.05).to_degrees();
    for a in text.lines().filter_map(|l| l.split(' ').find_map(|w| w.strip_prefix('A'))) {
        assert!(a.parse::<f64>().unwrap().abs() <= limit + 1e-3, "A{a}");
    }
    // Under two turns between the limits: one turn back each time.
    assert_eq!(rewinds_in(&text).len(), 5, "{text}");
    assert!(largest_deviation(&turning_cuts(&text), &[planned], &tight) <= post.tolerance + 1e-3);
    let minutes: f64 = written_cuts(&text).iter().map(|c| c.minutes).sum();
    close(minutes, planned.stats().cut_minutes + 5.0 * (1.0 / 500.0 + 1.0 / 250.0), 2e-3 * minutes);
    let rb = readback5(&text, &program, &post, &tight).unwrap();
    assert!(rb.within(), "{rb:?}");
    close(rb.cut_minutes, minutes, 1e-6 * minutes);
    // The writer says what the rewinds add, so the operation's minutes add up to the program's.
    let (again, written) = emit_program_report(&MachineProgram::from(&program), &post, &tight).unwrap();
    assert_eq!(again, text);
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].rewinds, 5);
    close(written[0].rewind_minutes, 5.0 * (1.0 / 500.0 + 1.0 / 250.0), 1e-9);
    close(planned.stats().cut_minutes + written[0].rewind_minutes, minutes, 2e-3 * minutes);

    // Without limits the helix still adds up its turns.
    let text = emit5(&program, &post, &Machine::four_axis_a()).unwrap();
    assert!(rewinds_in(&text).is_empty() && text.contains(" A1800."), "{text}");
    // A rewind needs room for a whole turn.
    let narrow = Machine::four_axis_a_with(Vec3::ZERO, false, Some((-deg(170.0), deg(170.0))));
    assert!(emit5(&program, &post, &narrow).is_err());
}

#[test]
fn dialect_switches_validate_and_default_to_the_old_output() {
    let post = dialect();
    let text = serde_json::to_string(&post).unwrap();
    assert_eq!(serde_json::from_str::<Post5>(&text).unwrap(), post);
    // Posts without the switches write none of them, and read back as they were.
    let old = serde_json::to_string(&Post::rs274()).unwrap();
    for key in ["tool_list", "safe_retract", "modal_motion", "blank_lines", "comment_chars", "operation_start", "stop_at_tool_change"] {
        assert!(!old.contains(key), "{old}");
    }
    let old5 = serde_json::to_string(&Post5::linuxcnc_4axis()).unwrap();
    assert!(!old5.contains("rewind") && !old5.contains("inverse_time_decimals") && !old5.contains("safe_rotate_first"), "{old5}");
    let broken = |f: &dyn Fn(&mut Post5)| {
        let mut p = dialect();
        f(&mut p);
        p.validate().is_err()
    };
    assert!(broken(&|p| p.base.comment_chars = Some("()".into())));
    assert!(broken(&|p| p.base.comment_chars = Some("x".repeat(100))));
    assert!(broken(&|p| p.base.safe_retract = Some(SafeRetract { lines: vec![], at_start: true, at_tool_change: false, at_end: false })));
    assert!(broken(&|p| p.base.operation_start = vec!["G93".into()]));
    assert!(broken(&|p| p.base.operation_end = vec!["G0 {nope}".into()]));
    assert!(broken(&|p| p.inverse_time_decimals = Some(13)));
    assert!(broken(&|p| p.rewind = Some(Rewind { distance: f64::NAN, retract_feed: 500.0, plunge_feed: 250.0 })));
    assert!(broken(&|p| p.rewind = Some(Rewind { distance: 25.0, retract_feed: 0.0, plunge_feed: 250.0 })));

    // A return home makes the axes it names unknown; other incremental moves are refused.
    let back = parse("G0 X1 Y1 Z1\nG28 G91 Z0.\nG90\nG0 X2 Y2\nG43 Z5. H1").unwrap();
    assert_eq!(back, vec![Move::Rapid { to: Vec3::new(1.0, 1.0, 1.0) }, Move::Rapid { to: Vec3::new(2.0, 2.0, 5.0) }]);
    assert!(parse("G28 G91 Z0.\nG0 X1 Y1 Z1").is_err());
    assert!(parse("G91 G0 X1").is_err());
    let a = [AxisLetter::A];
    let w = parse5("G0 X1 Y1 Z1 A0\nG28 G91 Z0.\nG90\nG0 A90.\nG0 X2 Y2\nG43 Z5. H1", &a).unwrap();
    assert_eq!(w.len(), 2);
    let JointWords::Rapid { linear, rotary, .. } = &w[1] else { panic!("{w:?}") };
    close3(*linear, Vec3::new(2.0, 2.0, 5.0), 1e-12);
    close(rotary[0], deg(90.0), 1e-12);
    assert!(parse5("G28 G91 Z0.\nG0 X1 Y1 Z1 A0", &a).is_err());
}

/// `op` with its coolant on.
fn with_coolant(op: MachineOperation) -> MachineOperation {
    match op {
        MachineOperation::Indexed(o) => MachineOperation::Indexed(IndexedOperation { coolant: true, ..o }),
        other => other,
    }
}

#[test]
fn coolant_can_come_on_after_the_first_turn_of_a() {
    let machine = Machine::four_axis_a();
    let side = |name: &str, a: f64| with_coolant(indexed_op(name, 3, vec![deg(a)], contour_path().moves));
    let program = MachineProgram { name: "sides".into(), operations: vec![side("side 0", 0.0), side("side 90", 90.0)], work_offset: None };
    // By default the coolant comes on before the index.
    let before = emit_program(&program, &dialect(), &machine).unwrap();
    assert!(before.contains("\nG54\nM8\nG0 A0.\nX0. Y-3.\n"), "{before}");
    // With the switch, right after it; the second side keeps the coolant on through its turn.
    let post = Post5 { coolant_after_turn: true, ..dialect() };
    post.validate().unwrap();
    let after = emit_program(&program, &post, &machine).unwrap();
    assert!(after.contains("\nG54\nG0 A0.\nM8\nG0 X0. Y-3.\nG43 Z10. H3\n"), "{after}");
    assert!(after.contains("\n(SIDE 90)\nG17 G90 G94\nG54\nG0 A90.\nX0. Y-3.\n"), "{after}");
    assert_eq!(after.matches("M8").count(), 1, "{after}");
    assert!(after.ends_with("G0 Z10.\n\nM9\nM5\nG28 G91 Z0.\nG90\nG28 G91 X0. Y0.\nG90\nM30\n"), "{after}");
    // Only the coolant line moves (the move after it states its G0 again, as after any line the
    // post writes).
    assert_eq!(before.replace("M8\n", ""), after.replace("M8\nG0 ", "").replace("M8\n", ""));
    assert_modal_with_points(&after);
    assert_reads_back(&after, &program, &post, &machine);

    // A wrapped helix starts with a turn of A too; its coolant follows that turn.
    let helix = MachineProgram { operations: vec![MachineOperation::Continuous(op5("helix", helix_of(1)))], ..program.clone() };
    let text = emit_program(&helix, &post, &machine).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let m8 = lines.iter().position(|l| *l == "M8").unwrap();
    assert!(lines[m8 - 1].starts_with("G0 A"), "{text}");
    assert!(!lines[..m8].iter().any(|l| l.starts_with("G1 ") || l.starts_with("G93")), "{text}");
    assert_reads_back(&text, &helix, &post, &machine);

    // Where nothing turns (a 3-axis machine), the coolant is written where it always was.
    let flat = Machine::three_axis();
    let level = MachineProgram { operations: vec![with_coolant(indexed_op("top", 3, vec![], contour_path().moves))], ..program.clone() };
    let plain = emit_program(&level, &dialect(), &flat).unwrap();
    assert_eq!(emit_program(&level, &post, &flat).unwrap(), plain);
    assert!(plain.contains("\nG54\nM8\nG0 X0. Y-3.\n"), "{plain}");

    // The switch is data: written only when on, read back as it was.
    let json = serde_json::to_string(&post).unwrap();
    assert!(json.contains("\"coolant_after_turn\":true"), "{json}");
    assert_eq!(serde_json::from_str::<Post5>(&json).unwrap(), post);
    assert!(!serde_json::to_string(&dialect()).unwrap().contains("coolant_after_turn"));
    assert!(!serde_json::to_string(&Post5::linuxcnc_4axis()).unwrap().contains("coolant_after_turn"));
}

/// With `coolant_after_turn`, through tool changes, an operation without coolant and a wrapped
/// cut that rewinds, with and without `safe_rotate_first`: every `M8` comes right after a turn
/// of A, never before it or after a cut, and every program reads back.
#[test]
fn coolant_after_turn_follows_every_index() {
    let machine = limited_a();
    let side = |name: &str, t: u32, a: f64| with_coolant(indexed_op(name, t, vec![deg(a)], contour_path().moves));
    let mut helix = op5("helix", helix_of(3));
    helix.tool = Tool::new(5, ToolKind::BallEndmill, 6.0, 20.0);
    let dry = match side("c", 4, 180.0) {
        MachineOperation::Indexed(o) => MachineOperation::Indexed(IndexedOperation { coolant: false, ..o }),
        other => other,
    };
    let program = MachineProgram {
        name: "p".into(),
        operations: vec![side("a", 3, 0.0), side("b", 4, 90.0), dry, MachineOperation::Continuous(helix), side("d", 5, 270.0)],
        work_offset: None,
    };
    for first in [true, false] {
        let post = Post5 { coolant_after_turn: true, safe_rotate_first: first, ..dialect() };
        let text = emit_program(&program, &post, &machine).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let on: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| **l == "M8").map(|(i, _)| i).collect();
        // The coolant comes on for a, for d after the dry c, and stays on through b and the helix.
        assert_eq!(on.len(), 3, "{text}");
        for i in on {
            assert!(lines[i - 1].starts_with("G0 A"), "`M8` after `{}`\n{text}", lines[i - 1]);
        }
        assert_reads_back(&text, &program, &post, &machine);
    }
}

/// Fails at the first rapid (`G0`, a return home or a move in machine coordinates) written
/// while inverse time (`G93`) is in force.
fn assert_no_rapid_in_inverse_time(text: &str) {
    let mut inverse = false;
    for line in text.lines() {
        let ws: Vec<&str> = line.split(' ').collect();
        if ws.contains(&"G93") {
            inverse = true;
        }
        if ws.contains(&"G94") {
            inverse = false;
        }
        assert!(!inverse || !ws.iter().any(|w| ["G0", "G00", "G28", "G30", "G53"].contains(w)), "a rapid in inverse time: `{line}`\n{text}");
    }
}

#[test]
fn feed_per_minute_comes_back_before_every_rapid() {
    let machine = Machine::four_axis_a();
    for post in [Post5::linuxcnc_4axis(), Post5::grblhal_4axis(), retracting(), dialect()] {
        for path in [helix(), helix_of(1), radial_drops(), linked_cuts()] {
            let text = emit5(&program5(path), &post, &machine).unwrap();
            assert_no_rapid_in_inverse_time(&text);
        }
    }
    // At the end of a wrapped cut: G94, then the rapid up.
    let mut up = helix_of(1);
    up.moves.pop();
    up.moves.push(Move5::Rapid { to: Pose::new(Vec3::new(10.0, 0.0, 0.0) + radial(0.0) * 30.0, radial(0.0)) });
    let text = emit5(&program5(up), &dialect(), &machine).unwrap();
    assert!(text.contains("\nG94\nG0 Z30.\n"), "{text}");
    // Rewinds too.
    let text = emit5(&program5(helix_of(5)), &dialect(), &limited_a()).unwrap();
    assert!(!rewinds_in(&text).is_empty());
    assert_no_rapid_in_inverse_time(&text);
}

/// Every `G0 A` of `text` is written with the tool at Z30 or above (a `G28 … Z0` sends it to
/// the machine's top); there are at least two.
fn assert_turns_at_z30_or_above(text: &str) {
    let mut z: Option<f64> = None;
    let mut turns = 0;
    for line in text.lines() {
        if line.starts_with("G28") {
            if line.contains("Z0") {
                z = Some(f64::INFINITY);
            }
            continue;
        }
        if let Some(v) = line.split(' ').find_map(|w| w.strip_prefix('Z')) {
            z = v.parse().ok();
        }
        if line.starts_with("G0 A") {
            turns += 1;
            assert!(z.is_some_and(|z| z >= 30.0 - 1e-9), "`{line}` at Z{z:?}\n{text}");
        }
    }
    assert!(turns >= 2, "{text}");
}

#[test]
fn a_rewind_turns_a_no_lower_than_the_operations_clearance() {
    // A post that turns A wherever the tool is (no safe lines, no rise to the clearance) and
    // draws the tool only 2 mm out of the cut: turned there, the tool would still be in the
    // groove for the whole turn. The turn is made at the operation's clearance (its rapids at
    // Z30) instead, where its own links turn A.
    let post = Post5 {
        safe_rotate: Vec::new(),
        safe_rotate_first: false,
        clearance_retract: false,
        rewind: Some(Rewind { distance: 2.0, retract_feed: 500.0, plunge_feed: 250.0 }),
        ..dialect()
    };
    let program = program5(helix_of(3));
    let text = emit5(&program, &post, &limited_a()).unwrap();
    assert!(!rewinds_in(&text).is_empty(), "{text}");
    assert_turns_at_z30_or_above(&text);
    // It comes back at rapid to 1 mm above the cut and feeds the rest of the way in.
    assert!(rewinds_in(&text).iter().all(|r| r.contains(&"Z21.") && r.contains(&"G1 Z20. F250.")), "{text}");

    // The post rises to Z25 to turn A while the operation's own rapids go round at Z30: a
    // rewind drawn out only to the post's height would turn the part 5 mm closer to the tool
    // than anything the operation does. It turns at the operation's clearance.
    let lower = Post5 { clearance_retract: true, clearance: Some(25.0), ..post };
    let text = emit5(&program, &lower, &limited_a()).unwrap();
    assert!(!rewinds_in(&text).is_empty(), "{text}");
    assert_turns_at_z30_or_above(&text);
}

#[test]
fn a_rewind_draws_out_to_the_operations_clearance_and_never_above_it() {
    // The cut is at Z20 and the operation's rapids go round at Z30. A rewind distance of 25
    // would have lifted the tool to Z45, above anything the operation asked for: a machine with
    // little Z travel above the part could not run it. The draw out ends at the operation's
    // clearance.
    for distance in [2.0, 25.0, 100.0] {
        let post = Post5 { rewind: Some(Rewind { distance, retract_feed: 500.0, plunge_feed: 250.0 }), ..dialect() };
        let text = emit5(&program5(helix_of(3)), &post, &limited_a()).unwrap();
        let rewinds = rewinds_in(&text);
        assert!(!rewinds.is_empty(), "{text}");
        assert!(rewinds.iter().all(|r| r.get(1) == Some(&"G0 Z30.")), "distance {distance}: {rewinds:?}");
        assert!(readback5(&text, &program5(helix_of(3)), &post, &limited_a()).unwrap().within(), "distance {distance}");
        for z in text.lines().filter(|l| !l.starts_with("G28")).filter_map(|l| l.split(' ').find_map(|w| w.strip_prefix('Z'))) {
            assert!(z.parse::<f64>().unwrap() <= 30.0 + 1e-9, "distance {distance}: Z{z} above the clearance\n{text}");
        }
    }
}

#[test]
fn a_rewind_stays_within_the_z_travel() {
    let post = dialect();
    let program = program5(helix_of(3));
    let travel = |top: f64| Machine { linear_limits: Some((Vec3::new(-100.0, -100.0, -100.0), Vec3::new(100.0, 100.0, top))), ..limited_a() };
    let text = emit5(&program, &post, &travel(100.0)).unwrap();
    assert!(rewinds_in(&text).first().is_some_and(|r| r.contains(&"G0 Z30.")), "{text}");
    // Little room above the operation's clearance (its rapids at Z30): the draw out still ends
    // there, and so does every block.
    let text = emit5(&program, &post, &travel(30.5)).unwrap();
    let rewinds = rewinds_in(&text);
    assert!(!rewinds.is_empty() && rewinds.iter().all(|r| r.contains(&"G0 Z30.")), "{text}");
    for z in text.lines().filter_map(|l| l.split(' ').find_map(|w| w.strip_prefix('Z'))) {
        assert!(z.parse::<f64>().unwrap() <= 35.0, "Z{z}\n{text}");
    }
    // A clearance above the travel: refused, naming the operation, the Z it needs and the travel.
    let high = Post5 { clearance: Some(40.0), ..post.clone() };
    let e = emit5(&program, &high, &travel(35.0)).unwrap_err().to_string();
    assert!(e.contains("`op`") && e.contains("Z up to 40.000") && e.contains("-100.000 to 35.000"), "{e}");
}

/// A wrapped helix of `turns` turns at radius 20 in steps of 30°, 1 mm along X a turn.
fn coarse_helix(turns: usize) -> Toolpath5 {
    let n = 12 * turns;
    let pose = |k: usize| {
        let th = TAU * k as f64 / 12.0;
        Pose::new(Vec3::new(k as f64 / 12.0, 0.0, 0.0) + radial(th) * 20.0, radial(th))
    };
    let mut moves = vec![Move5::Rapid { to: Pose::new(pose(0).up(10.0), radial(0.0)) }, Move5::Feed { to: pose(0), feed: 200.0 }];
    moves.extend((1..=n).map(|k| Move5::Feed { to: pose(k), feed: 300.0 }));
    moves.push(Move5::Feed { to: Pose::new(pose(n).up(10.0), pose(n).axis), feed: 1000.0 });
    Toolpath5 { moves }
}

#[test]
fn a_long_helix_keeps_its_angles_bounded_and_narrow_limits_say_so() {
    // 1700 turns add up to more than 10 000 radians of A.
    let post = Post5 { tolerance: 1.0, ..dialect() };
    let program = program5(coarse_helix(1700));
    let text = emit5(&program, &Post5 { rewind: None, ..post.clone() }, &Machine::four_axis_a()).unwrap();
    assert!(text.contains(" A612000. "), "the turns add up without end");
    let (text, written) = emit_program_report(&MachineProgram::from(&program), &post, &limited_a()).unwrap();
    for a in text.lines().filter_map(|l| l.split(' ').find_map(|w| w.strip_prefix('A'))) {
        assert!(a.parse::<f64>().unwrap().abs() <= 360.0, "A{a}");
    }
    assert_eq!(written[0].rewinds, rewinds_in(&text).len());
    assert!((845..=851).contains(&written[0].rewinds), "{written:?}");
    // Limits narrower than a turn: the cut would have to go on past them, and that is what the
    // refusal says (not a solution flip).
    let short = Machine::four_axis_a_with(Vec3::ZERO, false, Some((0.0, deg(359.0))));
    let e = emit5(&program5(helix_of(1)), &dialect(), &short).unwrap_err().to_string();
    assert!(e.contains("beyond its limits") && e.contains("less than a whole turn") && !e.contains("flip"), "{e}");
}

#[test]
fn an_operation_starts_at_the_turn_that_needs_the_fewest_rewinds() {
    // The first helix ends at A360, the upper limit; the second starts at the same angle of the
    // part, so it starts at A0 rather than at the limit and runs to 360° with no rewind.
    let post = dialect();
    let program = Program5 { name: "p".into(), operations: vec![op5("first", helix_of(1)), op5("second", helix_of(1))], work_offset: None };
    let (text, written) = emit_program_report(&MachineProgram::from(&program), &post, &limited_a()).unwrap();
    assert_eq!(written.iter().map(|w| w.rewinds).collect::<Vec<_>>(), [0, 0], "{text}");
    let second = &text[text.find("(SECOND)").unwrap()..];
    assert!(second.contains("\nG0 A0.\n"), "{second}");
    assert_on_path(&text, &program, &post, &limited_a());
    // Three turns within ±360°: one rewind.
    let (text, written) = emit_program_report(&MachineProgram::from(&program5(helix_of(3))), &post, &limited_a()).unwrap();
    assert_eq!(written[0].rewinds, 1, "{text}");
}

/// Two cuts along X at A = 0° and A = 40°, joined by a link at the clearance radius that turns
/// A 2° a rapid while it moves back along X, as a rotary toolpath's links do.
fn linked_cuts() -> Toolpath5 {
    let at = |x: f64, th: f64, r: f64| Pose::new(Vec3::new(x, 0.0, 0.0) + radial(th) * r, radial(th));
    let mut moves = vec![
        Move5::Rapid { to: at(0.0, 0.0, 30.0) },
        Move5::Feed { to: at(0.0, 0.0, 20.0), feed: 200.0 },
        Move5::Feed { to: at(20.0, 0.0, 20.0), feed: 400.0 },
        Move5::Rapid { to: at(20.0, 0.0, 30.0) },
    ];
    moves.extend((1..=20).map(|k| Move5::Rapid { to: at(20.0 - k as f64, deg(2.0 * k as f64), 30.0) }));
    moves.extend([
        Move5::Feed { to: at(0.0, deg(40.0), 20.0), feed: 200.0 },
        Move5::Feed { to: at(20.0, deg(40.0), 20.0), feed: 400.0 },
        Move5::Rapid { to: at(20.0, deg(40.0), 30.0) },
    ]);
    Toolpath5 { moves }
}

#[test]
fn a_link_that_only_turns_a_is_one_retract_and_one_turn() {
    let machine = Machine::four_axis_a();
    let program = program5(linked_cuts());
    for post in [dialect(), Post5::linuxcnc_4axis(), retracting()] {
        let text = emit5(&program, &post, &machine).unwrap();
        let turns: Vec<&str> = lines_with(&text, "G0 A");
        let expected = if post.base.trailing_decimal { ["G0 A0.", "G0 A40."] } else { ["G0 A0", "G0 A40"] };
        assert_eq!(turns, expected, "{text}");
        if let Some(first) = post.safe_rotate.first() {
            // One before each turn of A at most (the program's start, the link) and the end's.
            assert!(text.lines().filter(|l| l == first).count() <= 3, "{text}");
        }
        assert_on_path(&text, &program, &post, &machine);
    }
    // Where the rotary turns wherever the tool is, the link is followed as planned.
    let bare = Post5 { clearance_retract: false, safe_rotate: Vec::new(), ..Post5::linuxcnc_4axis() };
    let text = emit5(&program, &bare, &machine).unwrap();
    assert_eq!(lines_with(&text, "G0 A").len(), 21, "{text}");
}

#[test]
fn a_large_diagonal_raster_reads_back() {
    // A 45° raster over a 300 mm square at 0.5 mm stepover: every pass one long diagonal G1.
    let machine = Machine::four_axis_a();
    let post = Post5::linuxcnc_4axis();
    let step = 0.5 * 2f64.sqrt();
    let mut moves = Vec::new();
    let mut c = -300.0 + step * 0.5;
    while c < 300.0 {
        // The line x - y = c across the square.
        let (x0, x1) = (c.max(0.0), (300.0 + c).min(300.0));
        let (a, b) = (Vec3::new(x0, x0 - c, 0.0), Vec3::new(x1, x1 - c, 0.0));
        moves.push(Move5::Rapid { to: Pose::vertical(a + Vec3::Z * 5.0) });
        moves.push(Move5::Feed { to: Pose::vertical(a), feed: 300.0 });
        moves.push(Move5::Feed { to: Pose::vertical(b), feed: 1000.0 });
        moves.push(Move5::Rapid { to: Pose::vertical(b + Vec3::Z * 5.0) });
        c += step;
    }
    assert!(moves.len() > 3000);
    let program = program5(Toolpath5 { moves });
    let text = emit5(&program, &post, &machine).unwrap();
    let rb = readback5(&text, &program, &post, &machine).unwrap();
    assert!(rb.within(), "{rb:?}");
    close(rb.cut_length, rb.planned_cut_length, 1e-6 * rb.planned_cut_length);
}

/// A vertical lead as it is written: the plane word on the arc's own block
/// (`G19 G2 Y… Z… J… K…`, `G18 … I… K…`), `G17` again on the next XY arc and after the
/// operation, and every one read back in its own plane.
#[test]
fn vertical_arcs_are_written_with_their_plane_word_and_read_back() {
    let machine = Machine::three_axis();
    let post = Post5::linuxcnc_4axis();
    let moves = vec![
        Move::Rapid { to: Vec3::new(0.0, 10.3, 20.0) },
        Move::Rapid { to: Vec3::new(0.0, 10.3, 5.0) },
        Move::Feed { to: Vec3::new(0.0, 10.3, 0.15), feed: 50.0 },
        // Down a quarter in YZ into the start of a pass along -Y.
        Move::PlaneArc { to: Vec3::new(0.0, 10.0, -0.15), center: Vec3::new(0.0, 10.0, 0.15), plane: ArcPlane::Yz, ccw: false, feed: 1600.0 },
        Move::Feed { to: Vec3::new(0.0, 0.0, -0.15), feed: 1600.0 },
        // Round to the next pass in XY.
        Move::Arc { to: Vec3::new(5.0, 0.0, -0.15), center: Vec2::new(2.5, 0.0), ccw: true, feed: 1600.0 },
        Move::Feed { to: Vec3::new(5.0, 10.0, -0.15), feed: 1600.0 },
        // Up a quarter in YZ beyond its end.
        Move::PlaneArc { to: Vec3::new(5.0, 10.3, 0.15), center: Vec3::new(5.0, 10.0, 0.15), plane: ArcPlane::Yz, ccw: true, feed: 1600.0 },
        // And one in XZ.
        Move::Feed { to: Vec3::new(5.0, 10.3, -0.15), feed: 50.0 },
        Move::PlaneArc { to: Vec3::new(5.3, 10.3, 0.15), center: Vec3::new(5.3, 10.3, -0.15), plane: ArcPlane::Zx, ccw: true, feed: 1600.0 },
        Move::Rapid { to: Vec3::new(5.3, 10.3, 20.0) },
    ];
    let program = program_of(vec![indexed_op("face", 1, vec![], moves.clone())]);
    let text = emit_program(&program, &post, &machine).unwrap();
    assert!(text.contains("\nG19 G2 Y10 Z-0.15 J-0.3 K0 F1600\n"), "{text}");
    assert!(text.contains("\nG17 G3 X5 Y0 I2.5 J0\n"), "{text}");
    assert!(text.contains("\nG19 G3 Y10.3 Z0.15 J0 K0.3\n"), "{text}");
    assert!(text.contains("\nG18 G3 X5.3 Z0.15 I0.3 K0 F1600\n"), "{text}");
    // Back in G17 after the operation, before its end.
    let after = text.rsplit("G18 G3").next().unwrap();
    assert!(after.contains("\nG17\n"), "{text}");
    // Read as written: each arc in its plane about its own centre.
    let words = parse5(&text, &machine.letters()).unwrap();
    let plane_arcs: Vec<_> = words
        .iter()
        .filter_map(|w| if let JointWords::PlaneArc { center, plane, ccw, .. } = w { Some((*center, *plane, *ccw)) } else { None })
        .collect();
    assert_eq!(plane_arcs.len(), 3, "{plane_arcs:?}");
    assert_eq!((plane_arcs[0].1, plane_arcs[0].2), (ArcPlane::Yz, false));
    close3(plane_arcs[0].0, Vec3::new(0.0, 10.0, 0.15), 1e-9);
    assert_eq!((plane_arcs[2].1, plane_arcs[2].2), (ArcPlane::Zx, true));
    close3(plane_arcs[2].0, Vec3::new(5.3, 10.3, -0.15), 1e-9);
    let rb = readback_program(&text, &program, &post, &machine).unwrap();
    assert!(rb.within(), "{rb:?}");
    close(rb.cut_length, rb.planned_cut_length, 1e-3);
    // The quarter arcs are as long as they are round: a quarter of 2π·0.3 each.
    let planned = Toolpath { moves }.stats();
    close(planned.cut_length, 3.0 * 0.15 * std::f64::consts::PI + 4.85 + 10.0 + 2.5 * std::f64::consts::PI + 10.0 + 0.3, 1e-9);
    // A post that writes no arcs writes lines within its tolerance, in G17 throughout.
    let lines_only = Post5 { base: Post { arcs: false, ..post.base.clone() }, ..post.clone() };
    let text = emit_program(&program, &lines_only, &machine).unwrap();
    assert!(!text.contains("G19") && !text.contains("G18") && !text.contains("G2 ") && !text.contains("G3 "), "{text}");
    assert!(readback_program(&text, &program, &lines_only, &machine).unwrap().within());
    // An arc without its plane's offsets is refused when read.
    assert!(parse5("G0 X0 Y0 Z0\nG19 G2 Y1 Z1 I1 F100\n", &[]).is_err());
}

/// Where the written moves crowd (the turns of a small helix, a written arc's many chords), a
/// read-back point is measured against the pieces near it, not every piece within the reach,
/// and finds the same deviations as holding every point against every piece in reach.
#[test]
fn readback_measures_crowded_pieces_near_each_point_only() {
    use crate::readback5::directed_counted;
    use std::f64::consts::PI;
    let z = Vec3::new(0.0, 0.0, 1.0);
    let tilted = Vec3::new(0.0, 0.05f64.sin(), 0.05f64.cos());
    // A helix 2 mm across rising 0.05 mm a turn, 60 turns, in pieces about 0.03 mm long.
    let per_turn = 200;
    let helix = |t: f64, r: f64| Vec3::new(r * (TAU * t).cos(), r * (TAU * t).sin(), 0.05 * t);
    // And, away from it where nothing crowds, a line.
    let line = |x: f64| Pose { tip: Vec3::new(x, 0.0, 1.0), axis: z };
    let to: Vec<(Pose, Pose)> = (0..60 * per_turn)
        .map(|k| {
            let (t0, t1) = (f64::from(k) / f64::from(per_turn), f64::from(k + 1) / f64::from(per_turn));
            (Pose { tip: helix(t0, 1.0), axis: z }, Pose { tip: helix(t1, 1.0), axis: z })
        })
        .chain((0..10).map(|k| (line(f64::from(10 + k)), line(f64::from(11 + k)))))
        .collect();
    let (reach, allowed_tip, allowed_axis) = (1.0, 0.02, 0.01);
    // Every point against every piece in reach.
    let brute = |from: &[Pose]| {
        from.iter()
            .map(|p| {
                to.iter()
                    .filter_map(|(x, y)| {
                        let d = p.tip.dist_to_segment(x.tip, y.tip);
                        let e = arc_distance(p.axis, x.axis, y.axis);
                        (d <= reach).then_some(((d / allowed_tip).max(e / allowed_axis), d, e))
                    })
                    .min_by(|u, v| u.partial_cmp(v).unwrap())
                    .map_or((reach, PI), |(_, d, e)| (d, e))
            })
            .fold((0.0f64, 0.0f64), |(t, a), (d, e)| (t.max(d), a.max(e)))
    };
    // Points just off the helix, one in a hundred farther off than the near cells, one in
    // seven with its axis tilted (every piece in reach then scores the same), and along the line.
    let from: Vec<Pose> = (0..3000)
        .map(|k| {
            let r = if k % 100 == 0 { 1.4 } else { 1.003 };
            Pose { tip: helix(f64::from(k) * 0.0197, r), axis: if k % 7 == 0 { tilted } else { z } }
        })
        .chain((0..100).map(|k| Pose { tip: line(10.0 + f64::from(k) * 0.1).tip + Vec3::new(0.0, 0.004 * f64::from(k % 3), 0.0), axis: z }))
        .collect();
    let (worst, measured) = directed_counted(&from, &to, reach, allowed_tip, allowed_axis).unwrap();
    assert_eq!(worst, brute(&from));
    assert!(worst.0 > 0.3 && worst.1 > 0.04, "{worst:?}");
    assert!(measured < 400 * from.len(), "{measured} pieces measured for {} points", from.len());
    // And beyond the reach, the reach and a half turn.
    let lost = [Pose { tip: helix(3.0, 2.5), axis: z }];
    assert_eq!(directed_counted(&lost, &to, reach, allowed_tip, allowed_axis).unwrap().0, (reach, PI));
    assert_eq!(brute(&lost), (reach, PI));
}
