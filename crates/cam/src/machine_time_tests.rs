//! Tests of machine time: programs timed against a machine's rates, the warnings for blocks
//! the controller slows, and the dwell unit a post writes.

use solidcore_record::Vec3;

use crate::foundation_tests::close;
use crate::gcode5::{MachineOperation, MachineProgram, Post5, emit_program};
use crate::indexed::IndexedOperation;
use crate::kinematics::{AxisLetter, Machine};
use crate::machine_time::*;
use crate::readback5::readback_program;
use crate::*;

/// A small mill: 8000 mm/min across, 4000 up and down, A at 8000°/min, the spindle from 10000
/// to 24000 rpm with a six-second start.
fn rates() -> MachineRates {
    MachineRates {
        rapid_xyz_mm_min: 8000.0,
        rapid_z_mm_min: Some(4000.0),
        rapid_a_deg_min: Some(8000.0),
        max_feed_mm_min: Some(8000.0),
        max_feed_z_mm_min: Some(4000.0),
        max_a_deg_min: Some(8000.0),
        spindle_rpm: Some([10000.0, 24000.0]),
        spindle_start_seconds: Some(6.0),
        travel_mm: Some([300.0, 170.0, 130.0]),
        ..MachineRates::default()
    }
}

const PROGRAM: &str = "\
G90 G94 G91.1 G17 G21
M3 S5000
G0 X0. Y0. Z10. A0.
G0 X100.
G0 Z0.
G1 X110. F1000.
G93 G1 A90. F100.
G94 G1 X200. F12000.
G4 P1.5
G3 X210. Y10. I0. J10. F1000.
M5
M3 S12000
M30
";

fn line_of(text: &str, start: &str) -> usize {
    text.lines().position(|l| l.starts_with(start)).map_or(0, |i| i + 1)
}

#[test]
fn programs_are_timed_as_the_machine_runs_them() {
    let a = [AxisLetter::A];
    let t = machine_time(PROGRAM, &a, &rates(), DwellUnit::Seconds).unwrap();
    // Rapids: 100 mm across at 8000, 10 mm down at 4000; the first only places the axes.
    close(t.rapid_minutes, 100.0 / 8000.0 + 10.0 / 4000.0, 1e-12);
    // As written: the cut, the inverse-time turn, the fast cut, the dwell and the arc.
    let quarter = 10.0 * std::f64::consts::FRAC_PI_2;
    let written = 10.0 / 1000.0 + 1.0 / 100.0 + 90.0 / 12000.0 + 1.5 / 60.0 + quarter / 1000.0;
    close(t.written_cut_minutes, written, 1e-9);
    // As run: A held to 8000°/min, X to 8000 mm/min.
    let run = 10.0 / 1000.0 + 90.0 / 8000.0 + 90.0 / 8000.0 + 1.5 / 60.0 + quarter / 1000.0;
    close(t.cut_minutes, run, 1e-9);
    assert_eq!((t.slowed_blocks, t.rotary_over, t.feed_over), (2, 1, 1));
    assert_eq!(t.peak_rotary_deg_min.map(|(l, _)| l), Some(line_of(PROGRAM, "G93")));
    close(t.peak_rotary_deg_min.map_or(0.0, |(_, r)| r), 9000.0, 1e-6);
    assert_eq!(t.peak_feed_mm_min.map(|(l, _)| l), Some(line_of(PROGRAM, "G94 G1 X200")));
    // Two starts of the spindle, six seconds each; the first at a speed below its range.
    close(t.spindle_minutes, 12.0 / 60.0, 1e-12);
    assert_eq!(t.spindle_out_of_range, vec![(2, 5000.0)]);
    close(t.total_minutes, t.cut_minutes + t.rapid_minutes + t.spindle_minutes, 1e-12);
    assert_eq!(t.span_mm, [210.0, 10.0, 10.0]);
    let all = t.warnings.join("\n");
    assert_eq!(t.warnings.len(), 3, "{all}");
    assert!(all.contains("ask A to turn faster than its 8000°/min (up to 9000°/min at line 7)"), "{all}");
    assert!(all.contains("more than its top feed (up to 12000 mm/min at line 8)"), "{all}");
    assert!(all.contains("S5000 at line 2"), "{all}");

    // Less travel than the program spans.
    let short = MachineRates { travel_mm: Some([200.0, 170.0, 130.0]), ..rates() };
    let t = machine_time(PROGRAM, &a, &short, DwellUnit::Seconds).unwrap();
    assert!(t.warnings.iter().any(|w| w.contains("spans 210 mm along X, more than the machine's 200 mm")), "{:?}", t.warnings);
    // Without top rates nothing is slowed; without an A rapid rate a turn at rapid is untimed.
    let bare = MachineRates { rapid_xyz_mm_min: 8000.0, ..MachineRates::default() };
    let t = machine_time(PROGRAM, &a, &bare, DwellUnit::Seconds).unwrap();
    close(t.cut_minutes, t.written_cut_minutes, 1e-12);
    assert_eq!(t.slowed_blocks, 0);
    close(t.rapid_minutes, 110.0 / 8000.0, 1e-12);
    let turn = machine_time("G0 X0 Y0 Z0 A0\nG0 A90\n", &a, &bare, DwellUnit::Seconds).unwrap();
    assert!(turn.warnings.iter().any(|w| w.contains("1 rapids turn A, which is not timed")), "{:?}", turn.warnings);
    // A dwell written in milliseconds.
    let ms = machine_time(PROGRAM, &a, &rates(), DwellUnit::Milliseconds).unwrap();
    close(t.written_cut_minutes - ms.written_cut_minutes, 1.5 / 60.0 - 1.5e-3 / 60.0, 1e-9);
    // Nothing to time.
    let empty = machine_time("", &a, &rates(), DwellUnit::Seconds).unwrap();
    assert_eq!((empty.total_minutes, empty.span_mm, empty.warnings.len()), (0.0, [0.0; 3], 0));
}

#[test]
fn hostile_rates_and_programs_are_refused() {
    let a = [AxisLetter::A];
    let bad = [
        MachineRates { rapid_xyz_mm_min: 0.0, ..rates() },
        MachineRates { rapid_xyz_mm_min: f64::NAN, ..rates() },
        MachineRates { rapid_xyz_mm_min: f64::INFINITY, ..rates() },
        MachineRates { rapid_z_mm_min: Some(-1.0), ..rates() },
        MachineRates { rapid_a_deg_min: Some(f64::NAN), ..rates() },
        MachineRates { max_feed_mm_min: Some(1e9), ..rates() },
        MachineRates { max_feed_z_mm_min: Some(0.0), ..rates() },
        MachineRates { max_a_deg_min: Some(f64::NEG_INFINITY), ..rates() },
        MachineRates { spindle_rpm: Some([24000.0, 10000.0]), ..rates() },
        MachineRates { spindle_rpm: Some([f64::NAN, 10000.0]), ..rates() },
        MachineRates { spindle_rpm: Some([-1.0, 10000.0]), ..rates() },
        MachineRates { spindle_start_seconds: Some(-1.0), ..rates() },
        MachineRates { spindle_start_seconds: Some(f64::NAN), ..rates() },
        MachineRates { travel_mm: Some([300.0, 0.0, 130.0]), ..rates() },
        MachineRates { travel_mm: Some([300.0, f64::INFINITY, 130.0]), ..rates() },
    ];
    for r in &bad {
        assert!(r.validate().is_err(), "{r:?}");
        assert!(machine_time(PROGRAM, &a, r, DwellUnit::Seconds).is_err(), "{r:?}");
    }
    assert!(rates().validate().is_ok());
    for text in ["G0 X1.2.3", "G1 X1 Y1 Z1 A1 F0", "G0 X1 Y1 Z1 B1", "G91 G0 X1", "G93 G2 X1 Y1 I1 J0 F1", "G0 X(", "\u{0}\u{7f}é"] {
        assert!(machine_time(text, &a, &rates(), DwellUnit::Seconds).is_err(), "{text}");
    }
    // Unknown keys in a machine's rates are refused.
    let json = r#"{"rapid_xyz_mm_min": 8000, "rapid_xyz": 1}"#;
    assert!(serde_json::from_str::<MachineRates>(json).is_err());
    let back: MachineRates = serde_json::from_str(&serde_json::to_string(&rates()).unwrap()).unwrap();
    assert_eq!(back, rates());
}

/// A plunge with a dwell at the bottom, written through `post` and read back.
fn dwell_readback(post: &Post) -> (String, Readback) {
    let tool = Tool::new(1, ToolKind::Drill, 5.0, 20.0);
    let path = Toolpath {
        moves: vec![
            Move::Rapid { to: Vec3::new(0.0, 0.0, 5.0) },
            Move::Feed { to: Vec3::new(0.0, 0.0, -2.0), feed: 100.0 },
            Move::Dwell { seconds: 2.5 },
            Move::Feed { to: Vec3::new(0.0, 0.0, 5.0), feed: 100.0 },
        ],
    };
    let program = Program {
        name: "hole".into(),
        operations: vec![Operation { name: "hole".into(), tool, spindle_rpm: 12000.0, coolant: false, path }],
        work_offset: None,
    };
    let text = emit(&program, post).unwrap();
    let rb = readback(&text, &program, post).unwrap();
    (text, rb)
}

#[test]
fn dwells_are_read_in_the_unit_the_post_writes() {
    // In seconds, as a controller set to read `G4 P` in seconds wants it.
    let seconds = Post { dwell: "G4 P{seconds}".into(), trailing_decimal: true, ..Post::rs274() };
    assert_eq!(DwellUnit::of(&seconds).unwrap(), DwellUnit::Seconds);
    let (text, rb) = dwell_readback(&seconds);
    assert!(text.contains("\nG4 P2.5\n"), "{text}");
    close(rb.cut_minutes, rb.planned_cut_minutes, 1e-9);
    // In milliseconds, for a controller set to read it so: the read-back times it the same.
    let millis = Post { dwell: "G4 P{milliseconds}".into(), ..Post::rs274() };
    assert_eq!(DwellUnit::of(&millis).unwrap(), DwellUnit::Milliseconds);
    let (text, rb) = dwell_readback(&millis);
    assert!(text.contains("\nG4 P2500\n"), "{text}");
    close(rb.cut_minutes, rb.planned_cut_minutes, 1e-9);
    // A template that does not say how long, or says it twice, is refused.
    for dwell in ["G4 P1", "G4 P{seconds} (P{milliseconds})", ""] {
        let post = Post { dwell: dwell.into(), ..Post::rs274() };
        assert!(post.validate().is_err(), "{dwell}");
        assert!(DwellUnit::of(&post).is_err(), "{dwell}");
    }
    // The 4th-axis LinuxCNC post writes seconds.
    assert_eq!(DwellUnit::of(&Post5::linuxcnc_4axis().base).unwrap(), DwellUnit::Seconds);
}

#[test]
fn a_multi_axis_program_with_a_dwell_reads_back_in_milliseconds() {
    let tool = Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0);
    let path = Toolpath {
        moves: vec![
            Move::Rapid { to: Vec3::new(0.0, 0.0, 30.0) },
            Move::Feed { to: Vec3::new(0.0, 0.0, 18.0), feed: 200.0 },
            Move::Dwell { seconds: 0.5 },
            Move::Feed { to: Vec3::new(0.0, 0.0, 30.0), feed: 200.0 },
        ],
    };
    let op = IndexedOperation { name: "spot".into(), tool, spindle_rpm: 12000.0, coolant: true, rotary: vec![90f64.to_radians()], path };
    let program = MachineProgram { name: "spot".into(), operations: vec![MachineOperation::Indexed(op)], work_offset: None };
    let machine = Machine::four_axis_a();
    for (dwell, written) in [("G4 P{seconds}", "G4 P0.5"), ("G4 P{milliseconds}", "G4 P500")] {
        let mut post = Post5::linuxcnc_4axis();
        post.base.dwell = dwell.into();
        let text = emit_program(&program, &post, &machine).unwrap();
        assert!(text.lines().any(|l| l == written), "{text}");
        let rb = readback_program(&text, &program, &post, &machine).unwrap();
        assert!(rb.within(), "{rb:?}");
        close(rb.cut_minutes, rb.planned_cut_minutes, 1e-9);
        let t = machine_time(&text, &machine.letters(), &rates(), DwellUnit::of(&post.base).unwrap()).unwrap();
        close(t.written_cut_minutes, rb.cut_minutes, 1e-9);
    }
}

#[test]
fn rotary_rates_are_checked_outside_inverse_time_too() {
    // A post without inverse time writes a turn of A alone at an `F` in degrees a minute, and a
    // cut that moves and turns at an `F` along the linear axes: both can ask A for more than it
    // can turn, the controller slows them just the same, and the reply must say so.
    let a = [AxisLetter::A];
    let text = "G90 G94 G17 G21\nG0 X0. Y0. Z10. A0.\nG1 A90. F12000.\nG1 X10. A270. F1000.\nM30\n";
    let t = machine_time(text, &a, &rates(), DwellUnit::Seconds).unwrap();
    assert_eq!((t.slowed_blocks, t.rotary_over, t.feed_over), (2, 2, 0), "{t:?}");
    // The second asks for 180° in 0.01 minutes: 18000°/min.
    assert_eq!(t.peak_rotary_deg_min.map(|(l, _)| l), Some(4), "{t:?}");
    close(t.peak_rotary_deg_min.map_or(0.0, |(_, r)| r), 18000.0, 1e-6);
    close(t.cut_minutes, 90.0 / 8000.0 + 180.0 / 8000.0, 1e-12);
    let all = t.warnings.join("\n");
    assert!(all.contains("2 cutting blocks (the first at line 3) ask A to turn faster than its 8000°/min (up to 18000°/min at line 4)"), "{all}");
}

#[test]
fn the_feed_warning_names_a_block_that_is_slowed() {
    // A plunge at 5000 mm/min is slowed to Z's 4000; a diagonal at 7000 mm/min is not (X and Y
    // each move at 4950). The warning must point at the plunge, not at the faster diagonal.
    let a = [AxisLetter::A];
    let text = "G90 G94 G17 G21\nG0 X0. Y0. Z10. A0.\nG1 Z0. F5000.\nG1 X100. Y100. F7000.\nM30\n";
    let t = machine_time(text, &a, &rates(), DwellUnit::Seconds).unwrap();
    assert_eq!((t.slowed_blocks, t.feed_over), (1, 1), "{t:?}");
    // The highest feed asked for is still reported as it is.
    assert_eq!(t.peak_feed_mm_min.map(|(l, _)| l), Some(4), "{t:?}");
    let all = t.warnings.join("\n");
    assert!(
        all.contains("1 cutting blocks (the first at line 3) ask a linear axis for more than its top feed (up to 5000 mm/min at line 3)"),
        "{all}"
    );
}

#[test]
fn a_long_move_speeds_up_and_slows_down() {
    // 1000 mm at 100 mm/s, 2000 mm/s²: 2.5 mm to speed up and 2.5 to stop, 0.05 s each.
    close(trapezoid(1000.0, 0.0, 100.0, 0.0, 2000.0), 0.05 + 0.05 + 995.0 / 100.0, 1e-9);
    // Too short to reach the speed: a triangle, peak √(a·l).
    close(trapezoid(2.0, 0.0, 100.0, 0.0, 2000.0), 2.0 * (2000.0f64 * 2.0).sqrt() / 2000.0, 1e-9);
    assert_eq!(trapezoid(0.0, 0.0, 100.0, 0.0, 2000.0), 0.0);
    close(trapezoid(10.0, 0.0, 100.0, 0.0, f64::INFINITY), 0.1, 1e-12);
}

#[test]
fn blocks_are_timed_with_acceleration_when_the_rates_give_it() {
    let program = "G21\nT1 M6\nS12000 M3\nG0 X0. Y0. Z0.\nG1 X1000. F6000.\nG4 P2.\nM5\n";
    let plain =
        MachineRates { rapid_xyz_mm_min: 6000.0, spindle_start_seconds: Some(6.0), tool_change_seconds: Some(20.0), ..MachineRates::default() };
    let t = block_times(program, &[], &plain, DwellUnit::Seconds).unwrap();
    assert!(!t.accelerated);
    assert_eq!(t.lines, [4, 5, 6]);
    // The first block only says where the axes are; 1000 mm at 6000 mm/min is 10 s; the dwell 2.
    assert_eq!(t.seconds, [0.0, 10.0, 2.0]);
    assert_eq!(t.waits.iter().map(|w| (w.line, w.kind)).collect::<Vec<_>>(), [(2, WaitKind::ToolChange), (3, WaitKind::SpindleStart)]);
    close(t.total, 10.0 + 2.0 + 6.0 + 20.0, 1e-9);
    // With 2000 mm/s²: the hand calculation, 10.05 s.
    let accel = MachineRates { accel_mm_s2: Some(2000.0), ..plain.clone() };
    let t = block_times(program, &[], &accel, DwellUnit::Seconds).unwrap();
    assert!(t.accelerated);
    close(t.seconds[1], 10.05, 1e-9);
    // A square of short moves takes longer with it than without; a straight line of them does
    // not stop at their joins.
    let square = "G21\nG0 X0. Y0. Z0.\nG1 X5. F6000.\nY5.\nX0.\nY0.\n";
    let line = "G21\nG0 X0. Y0. Z0.\nG1 X5. F6000.\nX10.\nX15.\nX20.\n";
    let sum = |text: &str, r: &MachineRates| block_times(text, &[], r, DwellUnit::Seconds).unwrap().total;
    assert!(sum(square, &accel) > sum(square, &plain) + 0.1);
    close(sum(line, &accel), trapezoid(20.0, 0.0, 100.0, 0.0, 2000.0), 1e-9);
    // Without acceleration the cut time is the one `machine_time` gives.
    let mt = machine_time(PROGRAM, &[AxisLetter::A], &rates(), DwellUnit::Seconds).unwrap();
    let bt = block_times(PROGRAM, &[AxisLetter::A], &rates(), DwellUnit::Seconds).unwrap();
    close(bt.total, (mt.total_minutes) * 60.0, 1e-6);
    // Hostile accelerations are refused.
    for bad in [Some(0.0), Some(-1.0), Some(f64::NAN), Some(1e9)] {
        assert!(MachineRates { accel_mm_s2: bad, ..plain.clone() }.validate().is_err());
    }
    assert!(MachineRates { accel_z_mm_s2: Some(100.0), ..plain.clone() }.validate().is_err(), "Z's acceleration alone");
    assert!(MachineRates { tool_change_seconds: Some(-1.0), ..plain }.validate().is_err());
}

#[test]
fn returns_home_are_timed_up_to_the_home_and_back_down() {
    // A rewind as the post writes one: out of the cut, Z home (G28), A back two turns, and down
    // into the cut again. The trip to Z's home and back is time at rapid too.
    let text = "\
G90 G94 G17 G21
G0 X0. Y0. Z5. A0.
G1 Z-1. F100.
G28 G91 Z0.
G90
G0 A-720.
G0 X0. Y0.
G0 Z5.
G1 Z-1. F100.
";
    let a = [AxisLetter::A];
    // Home 50 mm above work zero: 51 up at 4000, then down 45 while A turns 720° at 8000°/min.
    let home = MachineRates { home_z_mm: Some(50.0), ..rates() };
    let t = machine_time(text, &a, &home, DwellUnit::Seconds).unwrap();
    close(t.rapid_minutes, 51.0 / 4000.0 + (45.0f64 / 4000.0).max(720.0 / 8000.0), 1e-12);
    assert_eq!(t.home_returns, 1);
    // Without the home's height, at least up to the highest Z the program reaches (5).
    let t = machine_time(text, &a, &rates(), DwellUnit::Seconds).unwrap();
    close(t.rapid_minutes, 6.0 / 4000.0 + 720.0 / 8000.0, 1e-12);
    assert_eq!(t.home_returns, 1);
    // A home below the program, or not finite, is refused.
    for z in [f64::NAN, f64::INFINITY, -1e9] {
        assert!(machine_time(text, &a, &MachineRates { home_z_mm: Some(z), ..rates() }, DwellUnit::Seconds).is_err(), "{z}");
    }
}

#[test]
fn spindle_speeds_outside_the_range_are_run_at_its_ends_unless_an_hss_edge_burns() {
    let range = [10000.0, 24000.0];
    // Below the range a speed is raised to the slowest, above it held to the fastest: a warning,
    // never a refusal, for carbide and for HSS in a material that does not burn it.
    assert_eq!(spindle_refusal([("Face", 12000.0, false), ("Pocket", 24000.0, false), ("Drill", 10000.0, false)], range, 1.3), None);
    assert_eq!(spindle_refusal([("Face", 12000.0, false), ("Drill", 3000.0, false), ("Bore", 30000.0, false)], range, 1.3), None);
    // An HSS tool in steel is refused only past 1.3 times its own speed.
    assert_eq!(spindle_refusal([("Drill", 8000.0, true), ("Bore", 30000.0, true)], range, 1.3), None);
    let r = spindle_refusal([("Face", 12000.0, true), ("Drill", 4500.0, true), ("Spot", 3000.0, true)], range, 1.3).unwrap();
    assert!(r.starts_with("spindle: 2 operations run an HSS tool"), "{r}");
    assert!(r.contains("`Drill`, asks for 4500 rpm"), "{r}");
    assert!(r.contains("carbide tool whose speed is at least 7692 rpm"), "{r}");
    // A speed that is not a number is outside every range.
    assert!(spindle_refusal([("Odd", f64::NAN, false)], range, 1.3).is_some());
    assert!(spindle_refusal([("Zero", 0.0, false)], range, 1.3).is_some());
    assert_eq!(spindle_refusal(std::iter::empty(), range, 1.3), None);
}

#[test]
fn feeds_above_the_axis_maximums_are_timed_clamped_with_a_warning() {
    let rates = MachineRates { rapid_xyz_mm_min: 5000.0, max_feed_mm_min: Some(1000.0), ..MachineRates::default() };
    // 100 mm at F2000: the controller holds it to 1000 mm/min, 0.1 min instead of 0.05.
    let text = "G90 G21\nG0 X0 Y0 Z5\nG1 X100 F2000\nM30\n";
    let t = machine_time(text, &[], &rates, DwellUnit::Seconds).unwrap();
    close(t.written_cut_minutes, 0.05, 1e-9);
    close(t.cut_minutes, 0.1, 1e-9);
    assert_eq!(t.feed_over, 1);
    assert!(t.warnings.iter().any(|w| w.contains("more than its top feed")), "{:?}", t.warnings);
}

#[test]
fn a_tool_change_is_a_stop_the_cycle_time_counts() {
    // An `M6` alone, a stop alone, and a change written as both: three changes.
    let text = "G90 G21\nT1 M6\nG0 X0 Y0 Z5\nG1 X10 F600\nT2 M6\nM5\n(change to tool 2)\nM0\nG0 X20\nM0\nG0 X30\nM30\n";
    assert_eq!(tool_change_lines(text).unwrap(), vec![2, 5, 10]);
    let mut rates = MachineRates { rapid_xyz_mm_min: 6000.0, ..MachineRates::default() };
    let t = machine_time(text, &[], &rates, DwellUnit::Seconds).unwrap();
    assert_eq!((t.tool_changes, t.tool_change_minutes), (3, 0.0));
    rates.tool_change_seconds = Some(30.0);
    let t2 = machine_time(text, &[], &rates, DwellUnit::Seconds).unwrap();
    close(t2.tool_change_minutes, 1.5, 1e-12);
    close(t2.total_minutes - t.total_minutes, 1.5, 1e-12);
    let b = block_times(text, &[], &rates, DwellUnit::Seconds).unwrap();
    assert_eq!(b.waits.iter().filter(|w| w.kind == WaitKind::ToolChange).count(), 3);
}

/// A probing program and a cutting program with a tool check, run one after the other.
const STOPPED: &str = "\
(PART 0 OF 1 - PROBE AND ZERO - REVISION R1)
(SC-STOP PROBE 1)
(MSG,PROBE - SHEET STEPS 1 TO 6)
M5
M0
(SC-STOP ZERO-CHECK)
G90 G54 G49
G0 X0. Y0.
G1 Z5. F300
(MSG,ZERO CHECK - THE PROBE TIP SHOULD BE 5 MM ABOVE X0 Y0)
M0
M30
(PART 1 OF 1)
T3 M6
G0 G43 Z20. H3
(SC-STOP TOOL-CHECK T3 GAP=10.)
G0 X-37. Y0.
G0 Z-12.87
(MSG,TOOL CHECK T3 - THE TIP SHOULD BE 10 MM ABOVE THE FIXED JAW TOP)
M0
G0 Z20.
M3 S5000
G1 X10. F600
(change to tool 4)
M0
G0 X20.
M30
";

#[test]
fn tagged_stops_are_classified() {
    let stops = stop_lines(STOPPED).unwrap();
    let kinds: Vec<(usize, StopKind, Option<u32>)> = stops.iter().map(|s| (s.line, s.kind, s.tool)).collect();
    assert_eq!(kinds, [(5, StopKind::Probe, None), (11, StopKind::ZeroCheck, None), (20, StopKind::ToolCheck, Some(3))]);
    assert_eq!(stops[2].gap, Some(10.0));
    assert!(stops[1].says.starts_with("ZERO CHECK"), "{:?}", stops[1]);
    // A post that drops `-` and `=` from its comments still tags them.
    assert_eq!(stop_tag("SCSTOP TOOLCHECK T12 GAP10."), Some((StopKind::ToolCheck, Some(12), Some(10.0))));
    assert_eq!(stop_tag("SC-STOP NOTHING"), None);
    assert_eq!(stop_tag("CHANGE TO TOOL 4"), None);
}

#[test]
fn a_probe_or_tool_check_stop_is_not_a_tool_change_but_an_untagged_stop_still_is() {
    // The `M6` at line 14 and the untagged `M0` at line 25: the three tagged stops are not changes.
    assert_eq!(tool_change_lines(STOPPED).unwrap(), vec![14, 25]);
}

#[test]
fn stop_minutes_are_added_to_the_total() {
    let rates = MachineRates { rapid_xyz_mm_min: 6000.0, ..MachineRates::default() };
    let t = machine_time(STOPPED, &[], &rates, DwellUnit::Seconds).unwrap();
    assert_eq!((t.stops, t.tool_changes), (3, 2));
    close(t.stop_minutes, 3.0 * STOP_MINUTES, 1e-12);
    close(t.total_minutes, t.cut_minutes + t.rapid_minutes + t.spindle_minutes + t.tool_change_minutes + t.stop_minutes, 1e-12);
    // The same program untagged: every stop a tool change, no stop minutes.
    let untagged: String = STOPPED.lines().filter(|l| !l.contains("SC-STOP")).map(|l| format!("{l}\n")).collect();
    let u = machine_time(&untagged, &[], &rates, DwellUnit::Seconds).unwrap();
    assert_eq!((u.stops, u.stop_minutes), (0, 0.0));
}
