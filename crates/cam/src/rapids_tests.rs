//! Tests of the rapid and travel checks of written programs: rapids through a fixture, the
//! stock and a holder's reach, a 4th-axis turn at a low height beside a square bar, the tool of
//! every block found per operation, and hostile input.

use solidcore_record::{Vec2, Vec3};

use crate::foundation_tests::block;
use crate::gcode5::{MachineOperation, MachineProgram, Post5, Rewind, emit_program};
use crate::indexed::IndexedOperation;
use crate::kinematics::Machine;
use crate::rapids::{RAPID_STEP, RapidCheck, RapidHitKind, check_rapids, check_rapids_staged, check_travel, check_travel_within};
use crate::rotary::rotary_pose;
use crate::stock::Stock;
use crate::*;

fn flat(number: u32, diameter: f64) -> Tool {
    Tool::new(number, ToolKind::FlatEndmill, diameter, 10.0)
}

/// A 3-axis operation of rapids alone, through `points`, then a short cut at the last.
fn rapids_op(name: &str, tool: Tool, points: &[Vec3]) -> MachineOperation {
    let mut moves: Vec<Move> = points.iter().map(|&to| Move::Rapid { to }).collect();
    if let Some(last) = points.last() {
        moves.push(Move::Feed { to: *last + Vec3::new(0.0, 0.0, -0.5), feed: 300.0 });
        moves.push(Move::Rapid { to: *last });
    }
    MachineOperation::Indexed(IndexedOperation {
        name: name.into(),
        tool,
        spindle_rpm: 12000.0,
        coolant: false,
        rotary: Vec::new(),
        path: Toolpath { moves },
    })
}

fn program(operations: Vec<MachineOperation>) -> MachineProgram {
    MachineProgram { name: "rapids".into(), operations, work_offset: None }
}

/// A part, a 20 mm square block with its top at Z 0, and a clamp beside it rising to Z 5.
fn part() -> Surface {
    block(Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 0.0))
}

fn part_and_clamp() -> Surface {
    let mut tris = part().triangles().to_vec();
    tris.extend_from_slice(block(Vec3::new(15.0, -5.0, -10.0), Vec3::new(20.0, 5.0, 5.0)).triangles());
    Surface::new(tris).unwrap()
}

fn check3(program: &MachineProgram, post: &Post5, stock: Option<&Stock>) -> RapidCheck {
    let machine = Machine::three_axis();
    let text = emit_program(program, post, &machine).unwrap();
    check_rapids(&text, program, post, &machine, &part_and_clamp(), &part(), stock, 0.01, RAPID_STEP).unwrap()
}

#[test]
fn a_rapid_through_a_clamp_is_found() {
    let post = Post5::linuxcnc_4axis();
    // Across the clamp at Z 3: through it. At Z 8: over it.
    let low = program(vec![rapids_op("low", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 3.0), Vec3::new(30.0, 0.0, 3.0)])]);
    let r = check3(&low, &post, None);
    assert!(!r.pass() && r.tools_known, "{r:?}");
    let hit = r.hits.iter().find(|h| h.kind == RapidHitKind::Part).unwrap();
    assert_eq!(hit.operation, Some(0));
    assert!(hit.depth > 1.9 && (12.0..=23.0).contains(&hit.at.x), "{hit:?}");
    let high = program(vec![rapids_op("high", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 8.0), Vec3::new(30.0, 0.0, 8.0)])]);
    let r = check3(&high, &post, None);
    assert!(r.pass(), "{r:?}");
    assert!(r.checked >= 3 && r.samples > 100, "every rapid is sampled every half millimetre: {r:?}");
    // Down onto the part top beside nothing, then a rapid down into the part: found.
    let into = program(vec![rapids_op("into", flat(1, 6.0), &[Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -2.0)])]);
    assert!(check3(&into, &post, None).hits.iter().any(|h| h.kind == RapidHitKind::Part));
}

#[test]
fn a_rapid_below_the_stock_top_over_the_stock_is_found() {
    let post = Post5::linuxcnc_4axis();
    let stock = Stock::Box { min: Vec3::new(-15.0, -15.0, -12.0), max: Vec3::new(15.0, 15.0, 1.0) };
    // Over the part's top by 0.5, but below the stock's top: clear of the part, in the stock.
    let skim = program(vec![rapids_op("skim", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 10.0), Vec3::new(-30.0, 0.0, 0.5), Vec3::new(0.0, 0.0, 0.5)])]);
    let r = check3(&skim, &post, None);
    assert!(r.pass(), "{r:?}");
    let r = check3(&skim, &post, Some(&stock));
    let hit = r.hits.iter().find(|h| h.kind == RapidHitKind::Stock).unwrap();
    assert!((hit.depth - 0.49).abs() < 1e-6, "{hit:?}");
    // Down beside the stock, the whole cutter clear of it: fine.
    let beside =
        program(vec![rapids_op("beside", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 10.0), Vec3::new(-18.5, 0.0, 10.0), Vec3::new(-18.5, 0.0, -5.0)])]);
    assert!(check3(&beside, &post, Some(&stock)).pass());
    let touching =
        program(vec![rapids_op("touching", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 10.0), Vec3::new(-17.0, 0.0, 10.0), Vec3::new(-17.0, 0.0, -5.0)])]);
    assert!(!check3(&touching, &post, Some(&stock)).pass());
    // Straight up out of a cut in the stock: through room the tool already fills. Up and
    // then across below the stock's top: through the stock.
    let out_of = |to: Vec3| {
        let moves = vec![
            Move::Rapid { to: Vec3::new(0.0, 0.0, 10.0) },
            Move::Feed { to: Vec3::new(0.0, 0.0, 0.5), feed: 300.0 },
            Move::Feed { to: Vec3::new(5.0, 0.0, 0.5), feed: 300.0 },
            Move::Rapid { to },
        ];
        let op = IndexedOperation {
            name: "out".into(),
            tool: flat(1, 6.0),
            spindle_rpm: 12000.0,
            coolant: false,
            rotary: Vec::new(),
            path: Toolpath { moves },
        };
        program(vec![MachineOperation::Indexed(op)])
    };
    let r = check3(&out_of(Vec3::new(5.0, 0.0, 10.0)), &post, Some(&stock));
    assert!(r.pass(), "{r:?}");
    let r = check3(&out_of(Vec3::new(9.0, 0.0, 0.8)), &post, Some(&stock));
    assert!(r.hits.iter().any(|h| h.kind == RapidHitKind::Stock), "{r:?}");
}

#[test]
fn a_holder_over_a_tall_clamp_is_found() {
    let post = Post5::linuxcnc_4axis();
    let mut tool = flat(1, 6.0);
    tool.stickout = Some(30.0);
    tool.holder = vec![HolderSection { diameter: 40.0, length: 30.0 }];
    let mut tris = part().triangles().to_vec();
    tris.extend_from_slice(block(Vec3::new(15.0, -5.0, -10.0), Vec3::new(20.0, 5.0, 40.0)).triangles());
    let obstacles = Surface::new(tris).unwrap();
    // Past the clamp 4 mm clear of the cutter; the holder, 33 mm up and 20 in radius, is not.
    let op = program(vec![rapids_op("past", tool.clone(), &[Vec3::new(-30.0, 12.0, 3.0), Vec3::new(30.0, 12.0, 3.0)])]);
    let machine = Machine::three_axis();
    let text = emit_program(&op, &post, &machine).unwrap();
    let r = check_rapids(&text, &op, &post, &machine, &obstacles, &part(), None, 0.01, RAPID_STEP).unwrap();
    assert!(r.hits.iter().any(|h| h.kind == RapidHitKind::Holder), "{r:?}");
    assert!(!r.hits.iter().any(|h| h.kind == RapidHitKind::Part), "{r:?}");
}

/// The tool of every block is its operation's: a small tool's rapid beside the clamp passes
/// although the face mill of the next operation would not fit there.
#[test]
fn each_rapid_is_checked_with_its_own_tool() {
    let post = Post5::linuxcnc_4axis();
    let ops = program(vec![
        rapids_op("small", flat(1, 6.0), &[Vec3::new(17.5, -30.0, 3.0), Vec3::new(17.5, -10.0, 3.0), Vec3::new(30.0, -10.0, 3.0)]),
        rapids_op("big", flat(2, 32.0), &[Vec3::new(-60.0, 0.0, 20.0), Vec3::new(-60.0, 0.0, 10.0)]),
    ]);
    let r = check3(&ops, &post, None);
    assert!(r.pass() && r.tools_known, "{r:?}");
    // The same path cut with the face mill is through the clamp.
    let ops =
        program(vec![rapids_op("big", flat(2, 32.0), &[Vec3::new(17.5, -30.0, 3.0), Vec3::new(17.5, -10.0, 3.0), Vec3::new(30.0, -10.0, 3.0)])]);
    assert!(check3(&ops, &post, None).hits.iter().any(|h| h.kind == RapidHitKind::Part));
    // A post whose tool changes write nothing for the first tool still names every block's.
    let r = check3(&program(vec![rapids_op("a", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 8.0)])]), &Post5::grblhal_4axis(), None);
    assert!(r.pass() && r.tools_known, "{r:?}");
}

/// A square bar 20 mm across (corners 14.1 mm from the A axis), the tool at radius 12, turned
/// A 0 to 45°: the corner sweeps through the tool. At radius 20, clear.
#[test]
fn an_a_turn_low_beside_a_square_bar_is_found() {
    let bar = block(Vec3::new(-20.0, -10.0, -10.0), Vec3::new(20.0, 10.0, 10.0));
    let machine = Machine::four_axis_a();
    let tool = flat(1, 6.0);
    let turn_at = |rho: f64| {
        let path = Toolpath5 {
            moves: vec![
                Move5::Rapid { to: rotary_pose(0.0, 0.0, 40.0) },
                Move5::Rapid { to: rotary_pose(0.0, 0.0, rho) },
                Move5::Rapid { to: rotary_pose(0.0, 45f64.to_radians(), rho) },
                Move5::Rapid { to: rotary_pose(0.0, 45f64.to_radians(), 40.0) },
            ],
        };
        program(vec![MachineOperation::Continuous(Operation5 {
            name: "turn".into(),
            tool: tool.clone(),
            spindle_rpm: 12000.0,
            coolant: false,
            path,
            plunge_feed: None,
        })])
    };
    // A post that turns where the tool is, without rising first.
    let mut post = Post5::linuxcnc_4axis();
    post.clearance_retract = false;
    let check = |p: &MachineProgram, post: &Post5, stock: Option<&Stock>| {
        let text = emit_program(p, post, &machine).unwrap();
        assert!(text.contains("A45"), "{text}");
        check_rapids(&text, p, post, &machine, &bar, &bar, stock, 0.01, RAPID_STEP).unwrap()
    };
    let r = check(&turn_at(12.0), &post, None);
    let hit = r.hits.iter().find(|h| h.kind == RapidHitKind::Turn).unwrap();
    assert!((hit.depth - (200f64.sqrt() + 0.01 - 12.0)).abs() < 0.05, "{hit:?}");
    assert!(r.hits.iter().any(|h| h.kind == RapidHitKind::Part), "the corner meets the cutter: {r:?}");
    assert!(check(&turn_at(20.0), &post, None).pass());
    // With round stock of radius 22 the turn at 20 is inside what the stock sweeps.
    let round = Stock::Cylinder { radius: 22.0, x_min: -25.0, x_max: 25.0 };
    assert!(check(&turn_at(20.0), &post, Some(&round)).hits.iter().any(|h| h.kind == RapidHitKind::Turn));
    assert!(check(&turn_at(23.0), &post, Some(&round)).pass());
    // A post that returns to the machine's top before a turn: the turn is made up there.
    let mut safe = post.clone();
    safe.safe_rotate = vec!["G28 G91 Z0.".into(), "G90".into()];
    let r = check(&turn_at(12.0), &safe, None);
    assert!(!r.hits.iter().any(|h| h.kind == RapidHitKind::Turn) && r.from_unknown > 0, "{r:?}");
}

#[test]
fn positions_beyond_the_travel_are_found() {
    let post = Post5::linuxcnc_4axis();
    let p = program(vec![rapids_op("a", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 8.0), Vec3::new(30.0, 0.0, 8.0)])]);
    let free = Machine::three_axis();
    let text = emit_program(&p, &post, &free).unwrap();
    assert_eq!(check_travel(&text, &free).unwrap(), None);
    let mut tight = Machine::three_axis();
    tight.linear_limits = Some((Vec3::new(-100.0, -100.0, -50.0), Vec3::new(100.0, 100.0, 5.0)));
    let t = check_travel(&text, &tight).unwrap().unwrap();
    let v = t.violation.unwrap();
    assert_eq!(v.axis, 'Z');
    assert!((v.value - 8.0).abs() < 1e-9 && v.line > 0, "{v:?}");
    tight.linear_limits = Some((Vec3::new(-100.0, -100.0, -50.0), Vec3::new(100.0, 100.0, 50.0)));
    let t = check_travel(&text, &tight).unwrap().unwrap();
    assert!(t.violation.is_none() && t.checked > 3, "{t:?}");
    tight.linear_limits = Some((Vec3::new(-20.0, -100.0, -50.0), Vec3::new(100.0, 100.0, 50.0)));
    assert_eq!(check_travel(&text, &tight).unwrap().unwrap().violation.map(|v| v.axis), Some('X'));
}

#[test]
fn soft_limits_are_checked_in_machine_coordinates_on_every_move() {
    let post = Post5::linuxcnc_4axis();
    let p = program(vec![rapids_op("a", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 8.0), Vec3::new(30.0, 0.0, 8.0)])]);
    let m = Machine::three_axis();
    let text = emit_program(&p, &post, &m).unwrap();
    // Soft limits X 0..300, Y 0..170, Z -130..0; work zero at machine (150, 85, -60): it fits.
    let (lo, hi) = (Vec3::new(0.0, 0.0, -130.0), Vec3::new(300.0, 170.0, 0.0));
    let t = check_travel_within(&text, &m, lo, hi, Vec3::new(150.0, 85.0, -60.0)).unwrap();
    assert!(t.violation.is_none() && t.checked > 3, "{t:?}");
    // Work zero 20 mm from X's low end: X -30 is at machine -10.
    let t = check_travel_within(&text, &m, lo, hi, Vec3::new(20.0, 85.0, -60.0)).unwrap();
    let v = t.violation.unwrap();
    assert_eq!(v.axis, 'X');
    assert!((v.value + 10.0).abs() < 1e-9 && v.low == 0.0, "{v:?}");
    // Work zero 5 mm under Z's top: the retract plane is above it.
    let v = check_travel_within(&text, &m, lo, hi, Vec3::new(150.0, 85.0, -5.0)).unwrap().violation.unwrap();
    assert_eq!(v.axis, 'Z');
    assert!(v.value > 0.0 && v.high == 0.0, "{v:?}");
    // Hostile limits are refused.
    assert!(check_travel_within(&text, &m, hi, lo, Vec3::ZERO).is_err());
    assert!(check_travel_within(&text, &m, lo, hi, Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
}

#[test]
fn hostile_rapid_checks_are_refused_without_panics() {
    let post = Post5::linuxcnc_4axis();
    let machine = Machine::three_axis();
    let p = program(vec![rapids_op("a", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 8.0)])]);
    let text = emit_program(&p, &post, &machine).unwrap();
    let s = part();
    for (tol, step) in [(f64::NAN, 0.5), (-1.0, 0.5), (0.01, 0.0), (0.01, f64::INFINITY), (0.01, 1e-9), (2.0, 0.5)] {
        assert!(check_rapids(&text, &p, &post, &machine, &s, &s, None, tol, step).is_err(), "{tol} {step}");
    }
    let bad = Stock::Box { min: Vec3::new(1.0, 0.0, 0.0), max: Vec3::new(0.0, 1.0, 1.0) };
    assert!(check_rapids(&text, &p, &post, &machine, &s, &s, Some(&bad), 0.01, 0.5).is_err());
    for junk in ["G0 X1 Y2 Z3\nG91 G0 X5\n", "G1 X1 Y0 Z0 F0\n", "G2 X1 Y1\n", "\u{0}(\n"] {
        assert!(check_rapids(junk, &p, &post, &machine, &s, &s, None, 0.01, 0.5).is_err(), "{junk:?}");
    }
    // A program that is not the one written: blocks of no known operation, checked with every tool.
    let other = "G0 X-30 Y0 Z8\nG0 X30 Y0 Z8\n";
    let r = check_rapids(other, &p, &post, &machine, &part_and_clamp(), &s, None, 0.01, 0.5).unwrap();
    assert!(!r.tools_known && r.pass() && r.checked == 2, "{r:?}");
    // A rapid of a kilometre is sampled within the limits; far beyond them it is refused.
    let far = "G0 X-500000 Y0 Z8\nG0 X500000 Y0 Z8\n";
    assert!(check_rapids(far, &p, &post, &machine, &s, &s, None, 0.01, 0.01).is_err());
    let empty = program(Vec::new());
    assert!(check_rapids("", &empty, &post, &machine, &s, &s, None, 0.01, 0.5).unwrap().pass());
    let mut limits = Machine::three_axis();
    limits.linear_limits = Some((Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0)));
    assert!(check_travel(&text, &limits).is_err());
    limits.linear_limits = Some((Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)));
    assert!(check_travel("G0 X1 Y2 Z3\nG91 G0 X5\n", &limits).is_err());
}

/// A rapid 0.5 over the part, below the top of a stock 1 mm above it: through the stock, unless
/// the operation before faced the whole top down to the part's. A stock faced only in part, or
/// a block of no known operation, is held against the stock as given.
#[test]
fn a_rapid_over_a_faced_stock_is_held_against_what_is_left() {
    let post = Post5::linuxcnc_4axis();
    let machine = Machine::three_axis();
    let stock = Stock::Box { min: Vec3::new(-15.0, -15.0, -12.0), max: Vec3::new(15.0, 15.0, 1.0) };
    let ops = program(vec![
        rapids_op("face", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 10.0), Vec3::new(-30.0, 0.0, 3.0)]),
        rapids_op("skim", flat(1, 6.0), &[Vec3::new(-30.0, 0.0, 10.0), Vec3::new(-30.0, 0.0, 0.5), Vec3::new(0.0, 0.0, 0.5)]),
    ]);
    let text = emit_program(&ops, &post, &machine).unwrap();
    let run = |stages: &[Option<Stock>]| {
        check_rapids_staged(&text, &ops, &post, &machine, &part_and_clamp(), &part(), Some(&stock), stages, &[], 0.01, RAPID_STEP)
    };
    let faced = stock.faced(stock.footprint(), 0.0);
    assert_eq!(faced, Stock::Box { min: Vec3::new(-15.0, -15.0, -12.0), max: Vec3::new(15.0, 15.0, 0.0) });
    assert!(run(&[Some(stock), Some(faced)]).unwrap().pass());
    let r = run(&[]).unwrap();
    assert!(r.hits.iter().any(|h| h.kind == RapidHitKind::Stock && h.operation == Some(1)), "{r:?}");
    // Half the top faced: the stock is kept as it was.
    let half = stock.faced([Vec2::new(-15.0, -15.0), Vec2::new(0.0, 15.0)], 0.0);
    assert_eq!(half, stock);
    assert!(!run(&[Some(stock), Some(half)]).unwrap().pass());
    // A bar's flat, a bottom out of the box or not a number: kept.
    let bar = Stock::Cylinder { radius: 5.0, x_min: 0.0, x_max: 1.0 };
    assert_eq!(bar.faced(bar.footprint(), 4.0), bar);
    for z in [-13.0, 2.0, f64::NAN] {
        assert_eq!(stock.faced(stock.footprint(), z), stock, "{z}");
    }
    // One stage per operation, each a valid stock.
    assert!(run(&[Some(stock)]).is_err());
    let bad = Stock::Box { min: Vec3::new(1.0, 0.0, 0.0), max: Vec3::new(0.0, 1.0, 1.0) };
    assert!(run(&[Some(stock), Some(bad)]).is_err());
    // A program that is not the one written: every block against the stock as given.
    let other = "G0 X-30 Y0 Z10\nG0 X-30 Y0 Z0.5\nG0 X0 Y0 Z0.5\n";
    let r = check_rapids_staged(other, &ops, &post, &machine, &part_and_clamp(), &part(), Some(&stock), &[Some(stock), Some(faced)], &[], 0.01, 0.5)
        .unwrap();
    assert!(!r.tools_known && r.hits.iter().any(|h| h.kind == RapidHitKind::Stock), "{r:?}");
}

/// A quarter arc of radius 100 from 90.7° to 0.7° (clockwise) peaks at Y 100 just after it
/// starts, between where 64 even samples look: beyond a travel ending at Y 99.995, found; within
/// one ending at Y 100.001, clear. Counter-clockwise the other way round, the same.
#[test]
fn an_arc_past_the_travel_between_samples_is_found() {
    let (a, b) = (90.7f64.to_radians(), 0.7f64.to_radians());
    let (s, e) = (Vec3::new(100.0 * a.cos(), 100.0 * a.sin(), 0.0), Vec3::new(100.0 * b.cos(), 100.0 * b.sin(), 0.0));
    let cw = format!("G90 G17\nG0 X{:.6} Y{:.6} Z0\nG2 X{:.6} Y{:.6} I{:.6} J{:.6} F300\n", s.x, s.y, e.x, e.y, -s.x, -s.y);
    let ccw = format!("G90 G17\nG0 X{:.6} Y{:.6} Z0\nG3 X{:.6} Y{:.6} I{:.6} J{:.6} F300\n", e.x, e.y, s.x, s.y, -e.x, -e.y);
    let mut m = Machine::three_axis();
    for text in [&cw, &ccw] {
        m.linear_limits = Some((Vec3::new(-200.0, -200.0, -50.0), Vec3::new(200.0, 99.995, 50.0)));
        let v = check_travel(text, &m).unwrap().unwrap().violation.unwrap_or_else(|| panic!("{text}"));
        assert!(v.axis == 'Y' && v.line == 3 && (v.value - 100.0).abs() < 1e-6, "{v:?}");
        m.linear_limits = Some((Vec3::new(-200.0, -200.0, -50.0), Vec3::new(200.0, 100.001, 50.0)));
        assert_eq!(check_travel(text, &m).unwrap().unwrap().violation, None, "{text}");
    }
}

/// Without a stock, an A turn is held to the part's radius where the tool is along X: over the
/// slender end of a stepped bar a turn clears it at a height the wide end would not allow.
#[test]
fn an_a_turn_is_held_to_the_part_where_the_tool_is_along_x() {
    let mut tris = block(Vec3::new(-20.0, -15.0, -15.0), Vec3::new(0.0, 15.0, 15.0)).triangles().to_vec();
    tris.extend_from_slice(block(Vec3::new(0.0, -10.0, -10.0), Vec3::new(20.0, 10.0, 10.0)).triangles());
    let bar = Surface::new(tris).unwrap();
    let machine = Machine::four_axis_a();
    let mut post = Post5::linuxcnc_4axis();
    post.clearance_retract = false;
    let turn_at = |x: f64, rho: f64| {
        let path = Toolpath5 {
            moves: vec![
                Move5::Rapid { to: rotary_pose(x, 0.0, 40.0) },
                Move5::Rapid { to: rotary_pose(x, 0.0, rho) },
                Move5::Rapid { to: rotary_pose(x, 45f64.to_radians(), rho) },
                Move5::Rapid { to: rotary_pose(x, 45f64.to_radians(), 40.0) },
            ],
        };
        let p = program(vec![MachineOperation::Continuous(Operation5 {
            name: "turn".into(),
            tool: flat(1, 4.0),
            spindle_rpm: 12000.0,
            coolant: false,
            path,
            plunge_feed: None,
        })]);
        let text = emit_program(&p, &post, &machine).unwrap();
        check_rapids(&text, &p, &post, &machine, &bar, &bar, None, 0.01, RAPID_STEP).unwrap()
    };
    // The small section reaches 14.14 mm from the axis, the large one 21.21 mm.
    assert!(!turn_at(10.0, 18.0).hits.iter().any(|h| h.kind == RapidHitKind::Turn), "over the small section");
    let r = turn_at(-10.0, 18.0);
    let hit = r.hits.iter().find(|h| h.kind == RapidHitKind::Turn).unwrap_or_else(|| panic!("{r:?}"));
    assert!((hit.depth - (450f64.sqrt() + 0.01 - 18.0)).abs() < 0.05, "{hit:?}");
    // Within the tool's radius of the step the large section counts.
    assert!(turn_at(1.0, 18.0).hits.iter().any(|h| h.kind == RapidHitKind::Turn));
}

/// An operation that cuts what the model does not show (no gouge check) retracts out of its
/// cut straight up: that retract is not held against the part, but its other rapids still are.
#[test]
fn a_retract_out_of_an_unchecked_cut_is_not_a_crash() {
    let machine = Machine::three_axis();
    let post = Post5::linuxcnc_4axis();
    // Over the clamp's side at Z 2 (through it), then down to 0.2 over the block: the cut goes
    // 0.3 into its top and the tool retracts.
    let p = program(vec![rapids_op("deburr", flat(1, 6.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.2)])]);
    let text = emit_program(&p, &post, &machine).unwrap();
    let checked = check_rapids_staged(&text, &p, &post, &machine, &part_and_clamp(), &part(), None, &[], &[false], 0.01, RAPID_STEP).unwrap();
    assert!(checked.hits.iter().any(|h| h.kind == RapidHitKind::Part), "{checked:?}");
    let unchecked = check_rapids_staged(&text, &p, &post, &machine, &part_and_clamp(), &part(), None, &[], &[true], 0.01, RAPID_STEP).unwrap();
    assert!(unchecked.pass(), "{unchecked:?}");
    // A rapid across through the clamp is held against it all the same.
    let across = program(vec![rapids_op("deburr", flat(1, 6.0), &[Vec3::new(12.0, 0.0, 2.0), Vec3::new(25.0, 0.0, 2.0)])]);
    let text = emit_program(&across, &post, &machine).unwrap();
    let r = check_rapids_staged(&text, &across, &post, &machine, &part_and_clamp(), &part(), None, &[], &[true], 0.01, RAPID_STEP).unwrap();
    assert!(r.hits.iter().any(|h| h.kind == RapidHitKind::Part), "{r:?}");
    // One flag per operation, or none.
    assert!(check_rapids_staged(&text, &across, &post, &machine, &part_and_clamp(), &part(), None, &[], &[true, true], 0.01, RAPID_STEP).is_err());
}

/// A peck drill rapids out of its hole and back down it to just above the depth it reached:
/// that way back is through room it has cut, so the stock does not count against it. A rapid
/// down past where the retract started, or down another line, still does.
#[test]
fn a_peck_rapid_back_into_its_own_hole_is_not_in_the_stock() {
    let machine = Machine::three_axis();
    let post = Post5::linuxcnc_4axis();
    let stock = Stock::Box { min: Vec3::new(-12.0, -12.0, -10.0), max: Vec3::new(12.0, 12.0, 0.0) };
    // The part is far off: only the stock is in the way.
    let hole = block(Vec3::new(100.0, 100.0, -10.0), Vec3::new(101.0, 101.0, -9.0));
    let pecks = |back: f64, x: f64| {
        let moves = vec![
            Move::Rapid { to: Vec3::new(0.0, 0.0, 10.0) },
            Move::Rapid { to: Vec3::new(0.0, 0.0, 2.0) },
            Move::Feed { to: Vec3::new(0.0, 0.0, -3.0), feed: 200.0 },
            Move::Rapid { to: Vec3::new(0.0, 0.0, 2.0) },
            Move::Rapid { to: Vec3::new(x, 0.0, 2.0) },
            Move::Rapid { to: Vec3::new(x, 0.0, back) },
            Move::Feed { to: Vec3::new(x, 0.0, -6.0), feed: 200.0 },
            Move::Rapid { to: Vec3::new(x, 0.0, 10.0) },
        ];
        let p = program(vec![MachineOperation::Indexed(IndexedOperation {
            name: "pecks".into(),
            tool: Tool::new(3, ToolKind::Drill, 5.0, 30.0),
            spindle_rpm: 6000.0,
            coolant: false,
            rotary: Vec::new(),
            path: Toolpath { moves },
        })]);
        let text = emit_program(&p, &post, &machine).unwrap();
        check_rapids(&text, &p, &post, &machine, &hole, &hole, Some(&stock), 0.01, RAPID_STEP).unwrap()
    };
    let ok = pecks(-2.5, 0.0);
    assert!(ok.pass(), "{ok:?}");
    let deeper = pecks(-3.5, 0.0);
    assert!(deeper.hits.iter().any(|h| h.kind == RapidHitKind::Stock && h.depth > 0.4), "{deeper:?}");
    let elsewhere = pecks(-2.5, 6.0);
    assert!(elsewhere.hits.iter().any(|h| h.kind == RapidHitKind::Stock), "{elsewhere:?}");
}

/// A level of a pocket or a roughing comes down at rapid through what the levels before it cut,
/// to just above the material left: that rapid is below the stock's top but in air the same
/// operation has cleared, so the stock does not count against it. The same rapid where nothing
/// was cut is in the stock.
#[test]
fn a_rapid_down_through_what_its_operation_cut_is_not_in_the_stock() {
    let post = Post5::linuxcnc_4axis();
    let stock = Stock::Box { min: Vec3::new(-15.0, -15.0, -12.0), max: Vec3::new(15.0, 15.0, 4.0) };
    let levels = |x: f64| {
        let v = Vec3::new;
        let moves = vec![
            Move::Rapid { to: v(-5.0, 0.0, 10.0) },
            Move::Rapid { to: v(-5.0, 0.0, 5.0) },
            Move::Feed { to: v(-5.0, 0.0, 2.0), feed: 100.0 },
            Move::Feed { to: v(5.0, 0.0, 2.0), feed: 300.0 },
            Move::Rapid { to: v(5.0, 0.0, 10.0) },
            Move::Rapid { to: v(x, 0.0, 10.0) },
            // Down at rapid to 1 mm above the level just cut.
            Move::Rapid { to: v(x, 0.0, 3.0) },
            Move::Feed { to: v(x, 0.0, 1.0), feed: 100.0 },
            Move::Feed { to: v(5.0, 0.0, 1.0), feed: 300.0 },
            Move::Rapid { to: v(5.0, 0.0, 10.0) },
        ];
        program(vec![MachineOperation::Indexed(IndexedOperation {
            name: "levels".into(),
            tool: flat(1, 6.0),
            spindle_rpm: 12000.0,
            coolant: false,
            rotary: Vec::new(),
            path: Toolpath { moves },
        })])
    };
    let r = check3(&levels(-5.0), &post, Some(&stock));
    assert!(r.pass(), "{r:?}");
    let r = check3(&levels(8.0), &post, Some(&stock));
    assert!(r.hits.iter().any(|h| h.kind == RapidHitKind::Stock && (h.at.x - 8.0).abs() < 1e-6), "{r:?}");
}

/// The same levels on a round bar, indexed at a quarter turn as well as at A0: the way down
/// into what the level before cut is along the tool's axis whatever the angle, so the bar does
/// not count against it there either; where nothing was cut it does.
#[test]
fn an_indexed_rapid_down_through_its_own_cut_is_not_in_the_stock_at_any_angle() {
    let machine = Machine::four_axis_a();
    let post = Post5::linuxcnc_4axis();
    let bar = Stock::Cylinder { radius: 16.0, x_min: -30.0, x_max: 30.0 };
    let far = block(Vec3::new(100.0, 100.0, -10.0), Vec3::new(101.0, 101.0, -9.0));
    let stock_hits = |a: f64, x: f64| {
        let v = Vec3::new;
        let moves = vec![
            Move::Rapid { to: v(-5.0, 0.0, 30.0) },
            Move::Rapid { to: v(-5.0, 0.0, 18.0) },
            Move::Feed { to: v(-5.0, 0.0, 15.0), feed: 100.0 },
            Move::Feed { to: v(5.0, 0.0, 15.0), feed: 300.0 },
            Move::Rapid { to: v(5.0, 0.0, 30.0) },
            Move::Rapid { to: v(x, 0.0, 30.0) },
            // Down at rapid to half a millimetre above the level just cut, inside the bar.
            Move::Rapid { to: v(x, 0.0, 15.5) },
            Move::Feed { to: v(x, 0.0, 14.0), feed: 100.0 },
            Move::Feed { to: v(5.0, 0.0, 14.0), feed: 300.0 },
            Move::Rapid { to: v(5.0, 0.0, 30.0) },
        ];
        let p = program(vec![MachineOperation::Indexed(IndexedOperation {
            name: "levels".into(),
            tool: flat(1, 6.0),
            spindle_rpm: 12000.0,
            coolant: false,
            rotary: vec![a],
            path: Toolpath { moves },
        })]);
        let text = emit_program(&p, &post, &machine).unwrap();
        let r = check_rapids(&text, &p, &post, &machine, &far, &far, Some(&bar), 0.01, RAPID_STEP).unwrap();
        r.hits.into_iter().filter(|h| h.kind == RapidHitKind::Stock).collect::<Vec<_>>()
    };
    for a in [0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI] {
        assert!(stock_hits(a, -5.0).is_empty(), "A {a}: {:?}", stock_hits(a, -5.0));
        let elsewhere = stock_hits(a, 8.0);
        assert!(elsewhere.iter().any(|h| (h.depth - 0.5).abs() < 0.02), "A {a}: {elsewhere:?}");
    }
}

/// A rewind draws the tool out of a wrapped cut, returns home, turns A a whole turn back and
/// comes down at rapid to just above where it left: that way down is the way it went out, so
/// the bar does not count against it, though only its end is known after the return home.
#[test]
fn a_rewinds_rapid_back_to_where_it_left_the_cut_is_not_in_the_stock() {
    let machine = Machine::four_axis_a_with(Vec3::ZERO, false, Some((-std::f64::consts::TAU, std::f64::consts::TAU)));
    let mut post = Post5::linuxcnc_4axis();
    post.safe_rotate = vec!["G28 G91 Z0.".into(), "G90".into()];
    post.rewind = Some(Rewind { distance: 25.0, retract_feed: 500.0, plunge_feed: 250.0 });
    let far = block(Vec3::new(100.0, 100.0, -10.0), Vec3::new(101.0, 101.0, -9.0));
    // Three turns at radius 15 into a bar of radius 17, 10 mm along X a turn.
    let n = 360;
    let at = |k: usize| rotary_pose(10.0 * k as f64 / 120.0, std::f64::consts::TAU * k as f64 / 120.0, 15.0);
    let mut moves = vec![Move5::Rapid { to: rotary_pose(0.0, 0.0, 25.0) }, Move5::Feed { to: at(0), feed: 200.0 }];
    moves.extend((1..=n).map(|k| Move5::Feed { to: at(k), feed: 300.0 }));
    moves.push(Move5::Feed { to: rotary_pose(30.0, 0.0, 25.0), feed: 1000.0 });
    let p = program(vec![MachineOperation::Continuous(Operation5 {
        name: "helix".into(),
        tool: flat(1, 4.0),
        spindle_rpm: 12000.0,
        coolant: false,
        path: Toolpath5 { moves },
        plunge_feed: None,
    })]);
    let text = emit_program(&p, &post, &machine).unwrap();
    assert!(text.contains("G0 A-360"), "a rewind: {text}");
    let bar = Stock::Cylinder { radius: 17.0, x_min: -5.0, x_max: 40.0 };
    let r = check_rapids(&text, &p, &post, &machine, &far, &far, Some(&bar), 0.01, RAPID_STEP).unwrap();
    assert!(!r.hits.iter().any(|h| h.kind == RapidHitKind::Stock), "{r:?}");
}

/// A drill fed to the apex of a blind hole's cone floor (to its full diameter) rapids out of it
/// straight up and back down to just above the depth it reached. The drill is held to its own
/// pointed tip, not a flat end of its diameter: the way out and back is in the hole it cut, not
/// 0.65 mm into the cone floor. The same drill rapided down off the hole's axis into the cone
/// floor is still a collision.
#[test]
fn a_drill_retract_out_of_its_own_cone_floor_is_not_through_the_part() {
    let machine = Machine::three_axis();
    let post = Post5::linuxcnc_4axis();
    let drill = Tool::new(3, ToolKind::Drill, 5.0, 30.0);
    let (r, tip) = (2.5, crate::drill::tip_length(&drill));
    let apex = Vec3::new(0.0, 0.0, -5.0 - tip);
    let n = 96;
    let rim = |i: usize| {
        let a = std::f64::consts::TAU * i as f64 / n as f64;
        Vec3::new(r * a.cos(), r * a.sin(), -5.0)
    };
    let floor =
        Surface::new((0..n).map(|i| crate::foundation_tests::facing([apex, rim(i), rim(i + 1)], Vec3::new(0.0, 0.0, 1.0))).collect()).unwrap();
    let run = |x: f64, back: f64| {
        let moves = vec![
            Move::Rapid { to: Vec3::new(0.0, 0.0, 10.0) },
            Move::Rapid { to: Vec3::new(0.0, 0.0, 2.0) },
            Move::Feed { to: apex, feed: 200.0 },
            Move::Rapid { to: Vec3::new(0.0, 0.0, 2.0) },
            Move::Rapid { to: Vec3::new(x, 0.0, 2.0) },
            Move::Rapid { to: Vec3::new(x, 0.0, back) },
            Move::Feed { to: Vec3::new(x, 0.0, back - 0.1), feed: 200.0 },
            Move::Rapid { to: Vec3::new(x, 0.0, 10.0) },
        ];
        let p = program(vec![MachineOperation::Indexed(IndexedOperation {
            name: "drill".into(),
            tool: drill.clone(),
            spindle_rpm: 6000.0,
            coolant: false,
            rotary: Vec::new(),
            path: Toolpath { moves },
        })]);
        let text = emit_program(&p, &post, &machine).unwrap();
        check_rapids(&text, &p, &post, &machine, &floor, &floor, None, 0.01, RAPID_STEP).unwrap()
    };
    let own = run(0.0, apex.z + 0.5);
    assert!(own.pass(), "{own:?}");
    let off_axis = run(1.0, apex.z + 0.2);
    assert!(off_axis.hits.iter().any(|h| h.kind == RapidHitKind::Part && h.depth > 0.1), "{off_axis:?}");
}
