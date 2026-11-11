//! Tests of the controller settings a post must agree with: the dwell unit a post writes, read
//! back in that unit.

use solidcore_record::Vec3;

use crate::foundation_tests::close;
use crate::gcode5::{MachineOperation, MachineProgram, Post5, emit_program};
use crate::indexed::IndexedOperation;
use crate::kinematics::Machine;
use crate::machine_time::*;
use crate::readback5::readback_program;
use crate::*;

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
    // Read as seconds, the same dwell would be a thousand times too long.
    let as_seconds: f64 = parse(&text).unwrap().iter().filter_map(|m| if let Move::Dwell { seconds } = m { Some(*seconds) } else { None }).sum();
    close(as_seconds, 2500.0, 1e-9);
    let unit = dwells_in(DwellUnit::Milliseconds, parse(&text).unwrap());
    assert!(unit.contains(&Move::Dwell { seconds: 2.5 }), "{unit:?}");
    // A template that does not say how long, or says it twice, is refused.
    for dwell in ["G4 P1", "G4 P{seconds} (P{milliseconds})", ""] {
        let post = Post { dwell: dwell.into(), ..Post::rs274() };
        assert!(post.validate().is_err(), "{dwell}");
        assert!(DwellUnit::of(&post).is_err(), "{dwell}");
    }
    // The built-in posts write seconds.
    assert_eq!(DwellUnit::of(&Post::rs274()).unwrap(), DwellUnit::Seconds);
    for name in Post5::BUILTIN {
        assert_eq!(DwellUnit::of(&Post5::builtin(name).unwrap().base).unwrap(), DwellUnit::Seconds, "{name}");
    }
}

#[test]
fn a_4th_axis_program_with_a_dwell_reads_back_in_either_unit() {
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
    }
}
