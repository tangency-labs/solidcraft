use crate::gcode::{CodeTable, MAX_INPUT_BYTES, MAX_LINE_BYTES, parse};
use crate::toolpath::{Diag, Interp, Move, MoveKind, Program};

fn run(text: &str) -> Program {
    parse(text, &CodeTable::default())
}

fn errors(p: &Program) -> Vec<&Diag> {
    p.diags.iter().filter(|d| d.is_error()).collect()
}

fn warnings(p: &Program) -> Vec<&Diag> {
    p.diags.iter().filter(|d| !d.is_error()).collect()
}

fn assert_close(a: [f64; 3], b: [f64; 3]) {
    for k in 0..3 {
        assert!((a[k] - b[k]).abs() < 1e-9, "{a:?} != {b:?}");
    }
}

fn only_move(p: &Program) -> &Move {
    assert_eq!(p.moves.len(), 1, "{:?}", p.moves);
    &p.moves[0]
}

#[test]
fn rapid_and_feed_words() {
    let p = run("G0 Z5\nG1 X10 Y20 Z-5 F300");
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 2);
    assert!(matches!(p.moves[0].kind, MoveKind::Rapid));
    assert_eq!(p.moves[0].feed, 0.0);
    assert_close(p.moves[0].tip, [0.0, 0.0, 5.0]);
    assert!(matches!(p.moves[1].kind, MoveKind::Feed));
    assert_close(p.moves[1].tip, [10.0, 20.0, -5.0]);
    assert_eq!(p.moves[1].feed, 300.0);
    assert_eq!(p.moves[1].axis, [0.0, 0.0, 1.0]);
    assert_eq!(p.moves[1].interp, Interp::Tcp);
    assert_eq!(p.moves[1].source, 2);
}

#[test]
fn modal_motion_and_no_axis_block() {
    let p = run("G1 X1\nX2\nF100\nG1\nZ0");
    assert_eq!(p.moves.len(), 3, "{:?}", p.moves);
    assert_close(p.moves[1].tip, [2.0, 0.0, 0.0]);
    assert!(matches!(p.moves[2].kind, MoveKind::Feed));
    assert!(p.diags.is_empty());
}

#[test]
fn incremental_distance_mode() {
    let p = run("G91 X10\nX10\nG90 X3");
    assert_close(p.moves[0].tip, [10.0, 0.0, 0.0]);
    assert_close(p.moves[1].tip, [20.0, 0.0, 0.0]);
    assert_close(p.moves[2].tip, [3.0, 0.0, 0.0]);
}

#[test]
fn inch_mode_converts_to_mm_and_feed() {
    let p = run("G20 G1 X1 Y-0.5 F10");
    assert_close(p.moves[0].tip, [25.4, -12.7, 0.0]);
    assert!((p.moves[0].feed - 254.0).abs() < 1e-9);
    let back = run("G21 G1 X1");
    assert_close(back.moves[0].tip, [1.0, 0.0, 0.0]);
}

#[test]
fn ccw_arc_with_ijk_is_counter_clockwise_about_its_centre() {
    let p = run("G1 X10 Y0\nG3 X0 Y10 I-10 J0");
    let m = &p.moves[1];
    match &m.kind {
        MoveKind::Arc { centre, normal, ccw, turns } => {
            assert!(*ccw);
            assert_eq!(*turns, 0);
            assert_close(*centre, [0.0, 0.0, 0.0]);
            assert_eq!(*normal, [0.0, 0.0, 1.0]);
        }
        other => panic!("expected an arc, got {other:?}"),
    }
    assert_close(m.tip, [0.0, 10.0, 0.0]);
}

#[test]
fn cw_arc_flag_and_absolute_centre() {
    let p = run("G1 X10 Y0\nG90.1 G2 X0 Y10 I0 J0");
    match &p.moves[1].kind {
        MoveKind::Arc { centre, ccw, .. } => {
            assert!(!*ccw);
            assert_close(*centre, [0.0, 0.0, 0.0]);
        }
        other => panic!("{other:?}"),
    }
    // Back to incremental centres (G91.1): the same offsets now move the centre.
    let q = run("G1 X10 Y0\nG91.1 G2 X0 Y10 I-10 J0");
    match &q.moves[1].kind {
        MoveKind::Arc { centre, .. } => assert_close(*centre, [0.0, 0.0, 0.0]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn r_form_arcs_minor_and_major() {
    let minor = run("G1 X10 Y0\nG3 X0 Y10 R10");
    match &minor.moves[1].kind {
        MoveKind::Arc { centre, .. } => assert_close(*centre, [0.0, 0.0, 0.0]),
        other => panic!("{other:?}"),
    }
    // A negative R is the major arc, whose centre lies on the other side of the chord.
    let major = run("G1 X10 Y0\nG3 X0 Y10 R-10");
    match &major.moves[1].kind {
        MoveKind::Arc { centre, .. } => assert_close(*centre, [10.0, 10.0, 0.0]),
        other => panic!("{other:?}"),
    }
    assert!(errors(&minor).is_empty() && errors(&major).is_empty());
}

#[test]
fn full_circle_and_helix() {
    let p = run("G1 X10 Y0\nG2 I-10 J0");
    assert_close(p.moves[1].tip, [10.0, 0.0, 0.0]);
    assert!(errors(&p).is_empty(), "{:?}", p.diags);
    let h = run("G1 X10 Y0 Z0\nG2 X0 Y10 Z5 I-10 J0");
    assert_close(h.moves[1].tip, [0.0, 10.0, 5.0]);
    assert!(errors(&h).is_empty());
}

#[test]
fn plane_xz_arc_uses_k_and_i() {
    let p = run("G18 G1 X10 Z0\nG2 X0 Z10 K0 I-10");
    match &p.moves[1].kind {
        MoveKind::Arc { centre, normal, .. } => {
            assert_close(*centre, [0.0, 0.0, 0.0]);
            assert_eq!(*normal, [0.0, 1.0, 0.0]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn tools_spindle_and_feed_recorded_per_move() {
    let p = run("T2 M6 S1200 M3\nG1 X1 F50\nM5\nG0 X2\nT3\nM6\nG1 X3");
    assert_eq!(p.moves[0].tool, 2);
    assert_eq!(p.moves[0].spindle, 1200.0);
    assert_eq!(p.moves[0].feed, 50.0);
    assert_eq!(p.moves[1].spindle, 0.0);
    assert_eq!(p.moves[2].tool, 3);
    assert!(p.diags.is_empty(), "{:?}", p.diags);
}

#[test]
fn comments_percent_line_numbers_and_case() {
    let p = run("%\nO100 (program)\nN10 g1 x1 ; trailing\n(a b) G1 Y2\n%");
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 2);
    assert_close(p.moves[1].tip, [1.0, 2.0, 0.0]);
}

#[test]
fn block_delete_skips_only_when_enabled() {
    let mut table = CodeTable::default();
    let skipped = parse("/G1 X1\nG1 X2", &table);
    assert_eq!(skipped.moves.len(), 2, "slash is ignored by default");
    table.block_delete = true;
    let p = parse("/G1 X1\nG1 X2", &table);
    assert_eq!(p.moves.len(), 1);
    assert_close(p.moves[0].tip, [2.0, 0.0, 0.0]);
}

#[test]
fn dwell_offsets_and_stops_are_accepted_silently() {
    let p = run("G4 P1\nG54 G1 X1\nG55 G0 X2\nG17 G80 G40 G49\nM0\nM1\nM7 M8 M9\nM30");
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 2);
}

#[test]
fn dwell_takes_its_x_word_as_a_time_and_makes_no_move() {
    // Fanuc-style G4 X1.5 is a dwell in seconds, not a move.
    let p = run("G1 X1\nG4 X1.5\nG4 P2");
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 1);
}

#[test]
fn position_is_unknown_after_home_so_the_next_move_is_a_jump() {
    let p = run("G1 X10\nG28\nG0 X20 Y20 Z5\nG1 X30");
    assert!(errors(&p).is_empty(), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 3);
    assert!(matches!(p.moves[1].kind, MoveKind::Jump), "{:?}", p.moves[1]);
    assert!(matches!(p.moves[2].kind, MoveKind::Feed));
    assert_close(p.moves[2].tip, [30.0, 20.0, 5.0]);
    // A relative move from the unknown home is an error, not a guess.
    let q = run("G28\nG91 X1");
    assert!(errors(&q).iter().any(|d| d.msg.contains("position is unknown")), "{:?}", q.diags);
}

#[test]
fn arc_turns_come_from_p_as_the_total_turns() {
    // LinuxCNC: P is the total number of turns, so P2 is one extra turn; P1 is the plain arc.
    let p = run("G1 X10 Y0\nG3 X0 Y10 I-10 J0 P2");
    match &p.moves[1].kind {
        MoveKind::Arc { turns, .. } => assert_eq!(*turns, 1),
        other => panic!("{other:?}"),
    }
    let plain = run("G1 X10 Y0\nG3 X0 Y10 I-10 J0 P1");
    match &plain.moves[1].kind {
        MoveKind::Arc { turns, .. } => assert_eq!(*turns, 0),
        other => panic!("{other:?}"),
    }
    let zero = run("G1 X10 Y0\nG3 X0 Y10 I-10 J0 P0");
    assert!(errors(&zero).iter().any(|d| d.msg.contains("at least 1")), "{:?}", zero.diags);
    let capped = run("G1 X10 Y0\nG3 X0 Y10 I-10 J0 P1002");
    assert!(errors(&capped).iter().any(|d| d.msg.contains("turns over")), "{:?}", capped.diags);
    assert_eq!(capped.moves.len(), 1);
}

#[test]
fn canned_cycle_error_makes_the_named_axes_unknown_until_the_next_full_move() {
    // The G81 line names X, Y and Z: they are unknown after it, so the later partial move makes no
    // move, and the full move after G80 does.
    let p = run("G1 X1\nG81 X1 Y1 R1 Z-1\nX2 Y2\nG80\nG1 X3\nG1 X3 Y0 Z0");
    let e = errors(&p);
    assert!(e.iter().any(|d| d.msg.contains("canned")), "{:?}", p.diags);
    assert!(e.iter().any(|d| d.msg.contains("axis words are ignored")), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 2, "{:?}", p.moves);
    assert!(matches!(p.moves[1].kind, MoveKind::Jump), "{:?}", p.moves[1]);
    assert_close(p.moves[1].tip, [3.0, 0.0, 0.0]);
}

#[test]
fn fusion_style_home_and_tool_length_sequence_has_no_errors() {
    // G28 with a Z word homes Z only; X and Y keep their values. The first move that gives all three
    // axes is a jump; G43 with a Z word supplies Z, as real posts write it.
    let text = "G90 G94 G17 G49 G40 G80\nG28 G91 Z0.\nG90\nG0 X10. Y10.\nG43 Z15. H1\nG1 Z-5. F100\n";
    let p = run(text);
    assert!(errors(&p).is_empty(), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 2, "{:?}", p.moves);
    assert!(matches!(p.moves[0].kind, MoveKind::Jump), "{:?}", p.moves[0]);
    assert_close(p.moves[0].tip, [10.0, 10.0, 15.0]);
    assert!(matches!(p.moves[1].kind, MoveKind::Feed));
    assert_close(p.moves[1].tip, [10.0, 10.0, -5.0]);
}

#[test]
fn incremental_move_on_an_unknown_axis_is_an_error() {
    let p = run("G28 G91 Z0.\nG91 Z-1");
    assert!(errors(&p).iter().any(|d| d.msg.contains("position is unknown")), "{:?}", p.diags);
    assert!(p.moves.is_empty());
}

#[test]
fn unknown_non_motion_codes_warn_and_motion_codes_error() {
    let p = run("G90 G94 G95 G64 G61.1 G15 G50 G69 G67 G98 G80\nG1 X1\nG22 P1\nG33 X3 K1");
    assert!(errors(&p).iter().any(|d| d.msg.contains("G33")), "{:?}", p.diags);
    assert!(warnings(&p).iter().any(|d| d.msg.contains("G22")), "{:?}", p.diags);
    assert!(!errors(&p).iter().any(|d| d.msg.contains("G94") || d.msg.contains("G98")), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 1);
}

#[test]
fn arc_block_without_words_moves_nothing() {
    let p = run("G1 X1\nG2 F200\nG3 M9\nG2 T2 M6");
    assert!(errors(&p).is_empty(), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 1);
}

#[test]
fn rounded_semicircle_is_accepted_by_the_r_form() {
    // The end point is 0.001 mm short of the diameter: inside the 0.002 mm tolerance.
    let p = run("G1 X0 Y0\nG3 Y10.001 R5");
    assert!(errors(&p).is_empty(), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 2);
}

#[test]
fn text_after_m30_is_ignored_with_one_warning() {
    let p = run("G1 X1\nM30\nG1 X2\nG1 X3\n%");
    assert_eq!(p.moves.len(), 1);
    assert_eq!(warnings(&p).iter().filter(|d| d.msg.contains("after M2 or M30")).count(), 1, "{:?}", p.diags);
}

#[test]
fn warnings_for_cutter_compensation_home_and_tool_length() {
    let p = run("G41 G1 X1\nG42 X2\nG43 H1 G1 X3\nG28\nG0 X5 Y5 Z5\nM10");
    assert!(errors(&p).is_empty(), "{:?}", p.diags);
    let w = warnings(&p);
    assert!(w.iter().any(|d| d.msg.contains("cutter compensation")));
    assert!(w.iter().any(|d| d.msg.contains("G28")));
    assert!(w.iter().any(|d| d.msg.contains("tool length")));
    assert!(w.iter().any(|d| d.msg.contains("M10")));
    // G41 and G42 still move: the path is shown uncompensated. The move after G30 is a jump.
    assert_eq!(p.moves.len(), 4);
    assert!(matches!(p.moves[3].kind, MoveKind::Jump));
    assert_eq!(w.iter().filter(|d| d.msg.contains("cutter")).count(), 2);
    assert!(w.iter().all(|d| d.line > 0));
}

#[test]
fn errors_have_line_and_column() {
    let p = run("G1 X1\n  G1 X2 #1=5");
    let e = errors(&p);
    assert_eq!(e.len(), 1, "{:?}", p.diags);
    assert_eq!(e[0].line, 2);
    assert_eq!(e[0].col, 9);
    assert!(e[0].msg.starts_with("variables"));
    assert_eq!(p.moves.len(), 1);
}

#[test]
fn unsupported_codes_are_errors() {
    for (text, want) in [
        ("G1 X1 G81 X1 Y1 R1 Z-1", "canned"),
        ("G10 X1", "G10 is not supported"),
        ("G1 X1 [1+2]", "variables"),
        ("G1 X1 L2", "word L"),
        ("G1 A10", "rotary"),
        ("M98 P100", "subprograms"),
        ("M99", "subprograms"),
        ("IF [1] GOTO 5", "IF is not supported"),
        ("WHILE", "WHILE is not supported"),
        ("G1 X1 @", "unexpected character"),
        ("G1 X1 (open", "unterminated comment"),
        ("G1 X", "word X has no value"),
        ("G1 X1e999", "word E"),
        ("G1 XNaN", "XNaN is not supported"),
        ("G1 X99999999999", "out of range"),
        ("G1 X1.000000000000000000000000000000000000001", "a number that is too long"),
        ("G1 F-5 X1", "feed rate is negative"),
        ("T1.5 M6", "whole number"),
        ("G1 G0 X1", "two motion codes"),
        ("G1 Y1 Z1 X1 Q2", "word Q"),
    ] {
        let p = run(text);
        let e = errors(&p);
        assert!(e.iter().any(|d| d.msg.contains(want)), "{text:?} gave {:?}", p.diags);
    }
}

#[test]
fn arc_errors() {
    let zero = run("G1 X1\nG2 X1 Y0 I0 J0");
    assert!(errors(&zero).iter().any(|d| d.msg.contains("radius is zero")), "{:?}", zero.diags);
    let mismatch = run("G1 X10 Y0\nG2 X0 Y10 I-5 J0");
    assert!(errors(&mismatch).iter().any(|d| d.msg.contains("off its circle")), "{:?}", mismatch.diags);
    let small = run("G1 X10\nG2 X0 Y0 R1");
    assert!(errors(&small).iter().any(|d| d.msg.contains("too small")), "{:?}", small.diags);
    let no_centre = run("G1 X10\nG2 X0 Y0");
    assert!(errors(&no_centre).iter().any(|d| d.msg.contains("no centre")));
    let both = run("G1 X10\nG2 X0 Y0 R5 I1");
    assert!(errors(&both).iter().any(|d| d.msg.contains("both R")));
    let full_r = run("G1 X10\nG2 X10 R5");
    assert!(errors(&full_r).iter().any(|d| d.msg.contains("full circle")));
}

#[test]
fn line_and_block_caps() {
    let long = format!("G1 X{}", "1".repeat(MAX_LINE_BYTES));
    let p = run(&long);
    assert!(errors(&p).iter().any(|d| d.msg.contains("4 KB")));
    assert!(p.moves.is_empty());
    let words: String = (0..65).map(|_| "X1 ").collect();
    let q = run(&words);
    assert!(errors(&q).iter().any(|d| d.msg.contains("64 words")), "{:?}", q.diags);
}

#[test]
fn input_over_64_mb_is_refused() {
    let big = "G1 X1\n".repeat(MAX_INPUT_BYTES / 6 + 2);
    assert!(big.len() > MAX_INPUT_BYTES);
    let p = run(&big);
    assert!(p.moves.is_empty());
    assert_eq!(p.diags.len(), 1);
    assert!(p.diags[0].is_error());
}

#[test]
fn ten_megabyte_line_is_an_error_not_a_panic() {
    let line = "G1 X1 ".repeat(10 * 1024 * 1024 / 6);
    let p = run(&line);
    assert!(p.moves.is_empty());
    assert!(errors(&p).iter().any(|d| d.msg.contains("4 KB")));
}

#[test]
fn moves_are_finite_and_serialise() {
    let p = run("G1 X1e5 Y-.5 Z+3.\nG2 I1 J1 X2 Y2\nG0 Z.25");
    for m in &p.moves {
        assert!(m.tip.iter().all(|x| x.is_finite()));
    }
    let json = serde_json::to_string(&p).expect("program serialises");
    let back: Program = serde_json::from_str(&json).expect("program reads back");
    assert_eq!(back, p);
}

#[test]
fn moves_cap_is_enforced_with_one_diag() {
    let text = "G1 X1\n".repeat(crate::gcode::MAX_MOVES + 5);
    let p = run(&text);
    assert_eq!(p.moves.len(), crate::gcode::MAX_MOVES);
    assert_eq!(errors(&p).len(), 1);
}

/// A deterministic generator for hostile input: xorshift, so the run is the same every time.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn hostile_random_bytes_never_panic() {
    let alphabet = b"GMTXYZIJKRFSPHNOgmxyz0123456789.-+()%;/ \t#[]=<>_abc\n\r";
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..3000 {
        let len = rng.below(200);
        let bytes: Vec<u8> =
            (0..len).map(|_| if rng.below(4) == 0 { (rng.next() & 0xff) as u8 } else { alphabet[rng.below(alphabet.len())] }).collect();
        let text = String::from_utf8_lossy(&bytes);
        let p = run(&text);
        assert!(p.moves.iter().all(|m| m.tip.iter().all(|x| x.is_finite())));
    }
}

#[test]
fn hostile_structured_words_never_panic() {
    let pieces = [
        "G0",
        "G1",
        "G2",
        "G3",
        "G91",
        "G90.1",
        "G91.1",
        "G20",
        "G21",
        "G41",
        "G43",
        "G49",
        "G53",
        "G28",
        "G81",
        "G10",
        "M3",
        "M5",
        "M6",
        "M98",
        "T1",
        "T99999999999",
        "F1e999",
        "X1e999",
        "I-0",
        "I9e18",
        "R0",
        "R-0.0000001",
        "Z-1e300",
        "S-1",
        "(",
        ")",
        ";",
        "#5",
        "[1]",
        "NaN",
        "inf",
        "X.",
        "X-",
        "1.2.3",
        "X00000000000000000000000000000000000000000000001",
        "K3",
        "J0.0000000001",
        "Y1",
        "Z2",
        "X1",
        "Y0",
        "G",
        "M",
    ];
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for _ in 0..3000 {
        let mut line = String::new();
        for _ in 0..rng.below(20) {
            line.push_str(pieces[rng.below(pieces.len())]);
            line.push(if rng.below(3) == 0 { '\n' } else { ' ' });
        }
        let p = run(&line);
        assert!(p.moves.iter().all(|m| m.tip.iter().all(|x| x.is_finite())));
        assert!(p.diags.len() <= 1000 + 1);
    }
}

#[test]
fn nan_and_exponent_text_is_rejected() {
    for text in ["G1 XNaN", "G1 Xinf", "G1 X1e999", "G1 X-1e400", "G1 X1E5"] {
        let p = run(text);
        assert!(p.moves.is_empty(), "{text}");
        assert!(!errors(&p).is_empty(), "{text}");
    }
}

#[test]
fn tool_is_none_until_m6() {
    // No tool is selected before the first M6, so a move before it is not taken for tool 0.
    let p = run("G1 X1");
    assert_eq!(p.moves[0].tool, u32::MAX);
    let tooled = run("T4 M6\nG1 X1");
    assert_eq!(only_move(&tooled).tool, 4);
    // M6 with no T word before it selects nothing either.
    let bare = run("M6\nG1 X1");
    assert_eq!(only_move(&bare).tool, u32::MAX);
}

#[test]
fn frame_and_unknown_codes_with_axis_words_are_errors_and_skip_the_block() {
    // G92 and the other frame codes would shift every later coordinate: an error, and no move from
    // their words. An unknown code with axis words would move: an error too.
    let p = run("G0 X5 Y5 Z5\nG1 Z-1 F100\nG92 X0 Y0 Z0");
    assert!(errors(&p).iter().any(|d| d.msg.contains("G92")), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 2, "{:?}", p.moves);
    assert_close(p.moves[1].tip, [5.0, 5.0, -1.0]);
    for text in ["G52 X10 Y10", "G68 X1", "G51 X2", "G28.1 X3", "G16 X4"] {
        let q = run(text);
        assert!(errors(&q).iter().any(|d| d.msg.contains("not supported") || d.msg.contains("frame")), "{text}: {:?}", q.diags);
        assert!(q.moves.is_empty(), "{text}: {:?}", q.moves);
    }
}

#[test]
fn moves_are_errors_while_a_frame_change_is_active() {
    // G16 makes the full move after it an error, so nothing is cut and nothing is guessed. G15
    // cancels it, and the next full move is a jump from the position the program now gives.
    let p = run("G1 X1 Y1 Z1\nG16\nG1 X10 Y45 Z0\nG15\nG1 X10 Y45 Z0");
    assert_eq!(errors(&p).len(), 2, "{:?}", p.diags);
    assert_eq!(p.moves.len(), 2, "{:?}", p.moves);
    assert!(matches!(p.moves[1].kind, MoveKind::Jump), "{:?}", p.moves[1]);
    assert_close(p.moves[1].tip, [10.0, 45.0, 0.0]);
}

#[test]
fn each_frame_transform_is_cancelled_on_its_own() {
    // G15 cancels only polar: rotation (G68) is still on, so the move stays an error until G69.
    let p = run("G68 R30\nG16\nG15\nG1 X1 Y1 Z1\nG69\nG1 X2 Y2 Z2");
    assert_eq!(p.moves.len(), 1, "{:?}", p.moves);
    assert!(matches!(p.moves[0].kind, MoveKind::Jump), "{:?}", p.moves[0]);
    assert_close(p.moves[0].tip, [2.0, 2.0, 2.0]);
    assert_eq!(errors(&p).len(), 3, "{:?}", p.diags);
}

#[test]
fn g92_is_cancelled_by_g92_1_and_g52_stays_until_the_end() {
    let p = run("G92 X0 Y0 Z0\nG92.1\nG1 X1 Y1 Z1");
    assert_eq!(errors(&p).len(), 1, "only the G92 line is an error: {:?}", p.diags);
    assert!(matches!(p.moves[0].kind, MoveKind::Jump), "{:?}", p.moves);
    // G52 has no cancel here: the moves after it stay errors until the program ends.
    let q = run("G52 X1\nG69\nG92.1\nG1 X1 Y1 Z1");
    assert!(q.moves.is_empty(), "{:?}", q.moves);
}

#[test]
fn hole_lines_in_cycle_mode_forget_axes() {
    // G81 forgets Z; the hole line X5 Y5 is an error and forgets X and Y too. After G80, the
    // partial moves make no move: nothing feeds from the old (0, 0, 10).
    let p = run("G0 X0 Y0 Z10\nG81 Z-1 R1 F100\nX5 Y5\nG80\nG0 Z10\nG1 X6");
    assert_eq!(p.moves.len(), 1, "{:?}", p.moves);
    assert!(!p.moves.iter().any(|m| matches!(m.kind, MoveKind::Feed)), "{:?}", p.moves);
}

#[test]
fn g84_is_a_canned_cycle_and_makes_no_feed() {
    let p = run("G0 X0 Y0 Z10\nG1 Z0 F100\nG84 X1 Y1 Z-5 R1\nX2 Y2\nG80");
    assert!(errors(&p).iter().any(|d| d.msg.contains("canned")), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 2, "{:?}", p.moves);
    assert!(!p.moves.iter().skip(2).any(|m| matches!(m.kind, MoveKind::Feed)));
}

#[test]
fn feed_that_starts_a_path_after_home_warns() {
    // After a Z-only home, the plunge gives Z: the path from there is not simulated, and the
    // program says so. No home Z is invented.
    let p = run("G28 G91 Z0\nG90 G0 X10 Y10\nG1 Z-5 F100");
    assert_eq!(p.moves.len(), 1, "{:?}", p.moves);
    assert!(matches!(p.moves[0].kind, MoveKind::Jump), "{:?}", p.moves[0]);
    assert!(warnings(&p).iter().any(|d| d.msg.contains("start") && d.msg.contains("not simulated")), "{:?}", p.diags);
}

#[test]
fn rapid_from_an_unknown_position_warns_like_a_feed() {
    // The Z rapid is the first full tip after the home: it starts a path the program cannot follow.
    let p = run("G28 G91 Z0\nG90 G0 X10 Y10\nG0 Z-5");
    assert_eq!(p.moves.len(), 1, "{:?}", p.moves);
    assert!(matches!(p.moves[0].kind, MoveKind::Jump), "{:?}", p.moves[0]);
    assert!(warnings(&p).iter().any(|d| d.msg.contains("start") && d.msg.contains("not simulated")), "{:?}", p.diags);
}

#[test]
fn g53_axis_words_make_axes_unknown_without_a_move() {
    // G53 gives machine coordinates the program does not know: no rapid to machine Z 0.
    let p = run("G0 X5 Y5 Z5\nG53 G0 Z0.\nG1 X7");
    assert!(errors(&p).is_empty(), "{:?}", p.diags);
    assert_eq!(p.moves.len(), 1, "{:?}", p.moves);
}
