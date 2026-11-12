//! Tests of setup sheets: the formats by extension, numbers and escaping, work zero and the
//! rotary convention in words, how low a toolpath reaches as written, and the pages themselves.

use std::f64::consts::{FRAC_PI_2, PI};

use solidcore_record::Vec3;

use crate::gcode5::{JointWords, MachineOperation, MachineProgram, Post5, emit_program, parse5};
use crate::indexed::IndexedOperation;
use crate::kinematics::{AxisLetter, Carrier, Joints, Machine, RotaryAxis};
use crate::multiaxis::{Move5, Operation5, Pose, Toolpath5};
use crate::setup_sheet::*;
use crate::{CommentStyle, Move, Tool, ToolKind, Toolpath};

#[test]
fn the_format_comes_from_the_extension() {
    assert_eq!(SheetFormat::from_path("C:/jobs/part.html").ok(), Some(SheetFormat::Html));
    assert_eq!(SheetFormat::from_path("part.HTM").ok(), Some(SheetFormat::Html));
    assert_eq!(SheetFormat::from_path("/x/y/part.md").ok(), Some(SheetFormat::Markdown));
    assert_eq!(SheetFormat::from_path("part.Markdown").ok(), Some(SheetFormat::Markdown));
    for bad in ["", "part", "part.txt", "part.html.exe", "C:/a.html/part", "C:\\a.md\\part", ".", "part."] {
        assert!(SheetFormat::from_path(bad).is_err(), "{bad}");
    }
}

#[test]
fn numbers_are_short_and_never_minus_zero() {
    assert_eq!(fmt_num(1.5, 3), "1.5");
    assert_eq!(fmt_num(12.0, 3), "12");
    assert_eq!(fmt_num(-0.0001, 3), "0");
    assert_eq!(fmt_num(1234.6, 0), "1235");
    assert_eq!(fmt_num(-2.26, 1), "-2.3");
    assert_eq!(fmt_num(f64::NAN, 3), "-");
    assert_eq!(fmt_num(f64::INFINITY, 0), "-");
    assert_eq!(fmt_num(1e300, 0).len(), 301);
}

#[test]
fn hostile_text_is_escaped() {
    let nasty = "<script>alert('x')</script> & \"q\"\n\u{0}|*_`[a](b)#~\\";
    let h = escape_html(nasty);
    assert!(!h.contains('<') && !h.contains('>') && !h.contains('"') && !h.contains('\'') && !h.contains('\n') && !h.contains('\u{0}'), "{h}");
    assert!(h.contains("&lt;script&gt;") && h.contains("&amp;"), "{h}");
    let m = escape_markdown(nasty);
    assert!(!m.contains('<') && !m.contains('\n') && !m.contains('\u{0}'), "{m}");
    // Every character Markdown gives a meaning follows a backslash.
    let chars: Vec<char> = m.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if matches!(c, '|' | '*' | '_' | '`' | '[' | ']' | '#' | '~') {
            assert_eq!(i.checked_sub(1).and_then(|j| chars.get(j)), Some(&'\\'), "{m}");
        }
    }
    assert_eq!(escape_markdown("plain 1.5 - 2"), "plain 1.5 - 2");
}

#[test]
fn work_zero_is_put_in_words() {
    let z = work_zero(Vec3::new(1.0, 2.0, 3.0), Vec3::new(-20.0, -20.0, -20.0), Vec3::new(20.0, 20.0, 7.0), None, None);
    let all = z.words.join(" ");
    assert!(all.contains("design point (1, 2, 3)"), "{all}");
    assert!(all.contains("X0 is in the middle of the part in X"), "{all}");
    assert!(all.contains("Y0 is in the middle of the part in Y"), "{all}");
    assert!(all.contains("Z0 is inside the part's extent, 20 mm from its bottom and 7 mm from its top"), "{all}");
    assert!(all.contains("40 x 40 x 27 mm"), "{all}");
    // On a corner, on the top; and away from the part.
    let z = work_zero(Vec3::ZERO, Vec3::new(0.0, 0.0, -10.0), Vec3::new(50.0, 30.0, 0.0), None, None);
    let all = z.words.join(" ");
    assert!(
        all.contains("X0 is on the part's left side") && all.contains("Y0 is on the part's front side") && all.contains("Z0 is on the part's top"),
        "{all}"
    );
    let z = work_zero(Vec3::ZERO, Vec3::new(5.0, -30.0, 2.0), Vec3::new(50.0, -10.0, 8.0), None, None);
    let all = z.words.join(" ");
    assert!(all.contains("X0 is 5 mm left of the part's left side"), "{all}");
    assert!(all.contains("Y0 is 10 mm behind the part's back side"), "{all}");
    assert!(all.contains("Z0 is 2 mm below the part's bottom"), "{all}");
    // On a 4th axis, Y0 Z0 is the axis.
    let z = work_zero(Vec3::ZERO, Vec3::new(-20.0, -10.0, -10.0), Vec3::new(20.0, 10.0, 10.0), Some('A'), Some(10.0));
    let all = z.words.join(" ");
    assert!(all.contains("Y0 Z0 is on the A axis centreline") && all.contains("reaches 10 mm from the A axis"), "{all}");
    assert!(!all.contains("Y0 is"), "{all}");
    assert_eq!(z.part_min, [-20.0, -10.0, -10.0]);
}

#[test]
fn the_a_axis_convention_is_put_in_words() {
    let a = Machine::four_axis_a();
    let note = rotary_note(&a.rotaries[0]);
    let all = note.words.join(" ");
    assert_eq!(note.letter, "A");
    assert!(all.contains("about +X through work zero"), "{all}");
    assert!(all.contains("counter-clockwise seen from the +X end"), "{all}");
    assert!(all.contains("A+90° brings the part's +Y side up"), "{all}");
    assert!(all.contains("top of the part moves towards -Y (towards the front"), "{all}");
    assert!(all.contains("no travel limits"), "{all}");
    assert!(note.limits_deg.is_none());
    // Reversed, and with limits: everything the other way round.
    let r = Machine::four_axis_a_with(Vec3::ZERO, true, Some((-2.0 * PI, 2.0 * PI)));
    let note = rotary_note(&r.rotaries[0]);
    let all = note.words.join(" ");
    assert!(all.contains("turns the table clockwise") && all.contains("reversed"), "{all}");
    assert!(all.contains("A+90° brings the part's -Y side up"), "{all}");
    assert!(all.contains("towards +Y"), "{all}");
    assert!(all.contains("from -360° to 360°"), "{all}");
    assert_eq!(note.limits_deg, Some([-360.0, 360.0]));
    // An axis off the coordinate directions, on the head, says so in numbers.
    let tilted = RotaryAxis::new(AxisLetter::B, Vec3::new(0.0, 1.0, 1.0), Vec3::new(0.0, 0.0, 100.0), Carrier::Head, None);
    let all = rotary_note(&tilted).words.join(" ");
    assert!(all.contains("tilts the spindle head about [0, 0.7071, 0.7071] through (0, 0, 100)"), "{all}");
}

#[test]
fn the_a_axis_words_agree_with_the_kinematics() {
    let named = |n: &str| match n {
        "+Y" => Vec3::new(0.0, 1.0, 0.0),
        "-Y" => Vec3::new(0.0, -1.0, 0.0),
        "+Z" => Vec3::new(0.0, 0.0, 1.0),
        _ => Vec3::new(0.0, 0.0, -1.0),
    };
    for (dir, reversed) in [(Vec3::X, false), (Vec3::X, true), (Vec3::new(-1.0, 0.0, 0.0), false), (Vec3::new(-1.0, 0.0, 0.0), true)] {
        let axis = RotaryAxis::new(AxisLetter::A, dir, Vec3::ZERO, Carrier::Table, None).reversed(reversed);
        let m = Machine { name: "t".into(), rotaries: vec![axis], linear_limits: None, work_origin: Vec3::ZERO };
        let words = rotary_note(&axis).words.join(" ");
        // The side the words say A+90° brings up is the side a tool along it is at A+90° for.
        let up = ["+Y", "-Y", "+Z", "-Z"].into_iter().find(|n| words.contains(&format!("brings the part's {n} side up"))).unwrap();
        let v = named(up);
        let j = crate::kinematics::inverse(&m, &Pose::new(v * 10.0, v), None).unwrap();
        assert!((j.rotary[0] - FRAC_PI_2).abs() < 1e-9, "{dir:?} {reversed}: {words} but A is {j:?}");
        // And the top of the part moves the way the words say as A rises.
        let towards = ["+Y", "-Y"].into_iter().find(|n| words.contains(&format!("moves towards {n}"))).unwrap();
        let top = crate::kinematics::table_point(&m, &[0.01], Vec3::new(0.0, 0.0, 10.0));
        assert!(top.y * named(towards).y > 0.0, "{dir:?} {reversed}: {words} but the top goes to {top:?}");
    }
}

#[test]
fn rotary_angles_are_one_angle_or_a_range() {
    let a = vec!["A".to_string()];
    assert_eq!(rotary_words(&a, &[(FRAC_PI_2, FRAC_PI_2)]), "A90°");
    assert_eq!(rotary_words(&a, &[(-PI, PI)]), "A -180° to 180°");
    assert_eq!(rotary_words(&a, &[(f64::INFINITY, f64::NEG_INFINITY)]), "");
    assert_eq!(rotary_words(&[], &[]), "");
    assert_eq!(rotary_words(&a, &[]), "");
}

fn path(poses: &[(Vec3, Vec3)]) -> Toolpath5 {
    let mut moves = Vec::new();
    for (i, (tip, axis)) in poses.iter().enumerate() {
        let to = Pose::new(*tip, *axis);
        moves.push(if i == 0 { Move5::Rapid { to } } else { Move5::Feed { to, feed: 500.0 } });
    }
    Toolpath5 { moves }
}

#[test]
fn reach_is_measured_as_the_program_writes_it() {
    // On the 4th axis the program's Z of a tip on the +Y side with the tool along +Y is its
    // distance from the axis, and A turns to 90°.
    let p = path(&[(Vec3::new(0.0, 0.0, 20.0), Vec3::Z), (Vec3::new(3.0, 10.0, 0.0), Vec3::Y), (Vec3::new(3.0, 12.0, 0.0), Vec3::Y)]);
    let a = Machine::four_axis_a();
    let r = reach(&a, &p, None).unwrap().unwrap();
    assert!((r.z_min - 10.0).abs() < 1e-9, "{r:?}");
    assert_eq!(r.rotary.len(), 1);
    assert!(r.rotary[0].0.abs() < 1e-9 && (r.rotary[0].1 - FRAC_PI_2).abs() < 1e-9, "{r:?}");
    assert!(r.last.is_some());
    // A reversed table turns to -90° for the same pose.
    let rev = Machine::four_axis_a_with(Vec3::ZERO, true, None);
    let r = reach(&rev, &p, None).unwrap().unwrap();
    assert!((r.rotary[0].0 + FRAC_PI_2).abs() < 1e-9, "{r:?}");
    // 3-axis: the lowest tip.
    let flat = path(&[(Vec3::new(0.0, 0.0, 5.0), Vec3::Z), (Vec3::new(1.0, 1.0, -3.5), Vec3::Z)]);
    let r = reach(&Machine::three_axis(), &flat, None).unwrap().unwrap();
    assert!((r.z_min + 3.5).abs() < 1e-12 && r.rotary.is_empty(), "{r:?}");
    // Nothing to reach, and hostile poses, are no panic.
    assert!(reach(&Machine::three_axis(), &Toolpath5 { moves: vec![Move5::Dwell { seconds: 1.0 }] }, None).unwrap().is_none());
    let bad = path(&[(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Z)]);
    assert!(reach(&Machine::three_axis(), &bad, None).is_err());
    let bad = path(&[(Vec3::ZERO, Vec3::ZERO)]);
    assert!(reach(&a, &bad, None).is_err());
    let down = path(&[(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0))]);
    assert!(reach(&Machine::three_axis(), &down, None).is_err());
}

fn sample(name: &str) -> SetupSheet {
    SetupSheet {
        job: name.into(),
        machine: "4-axis A".into(),
        post: "linuxcnc_4axis".into(),
        program_file: Some("part.tap".into()),
        work_offset: "G54".into(),
        work_zero: work_zero(Vec3::ZERO, Vec3::new(-20.0, -10.0, -10.0), Vec3::new(20.0, 10.0, 10.0), Some('A'), Some(10.0)),
        rotaries: vec![rotary_note(&Machine::four_axis_a().rotaries[0])],
        stock: None,
        tools: vec![
            SheetTool {
                number: 1,
                kind: "flat end mill".into(),
                description: format!("6 mm {name}"),
                diameter: 6.0,
                flutes: 3,
                flute_length: 15.0,
                stickout: Some(25.0),
                holder: vec![[34.0, 4.0], [34.0, 18.0]],
                z_min: Some(-2.5),
                operations: vec![format!("rough {name}")],
                ..SheetTool::default()
            },
            SheetTool {
                number: 2,
                kind: "ball end mill".into(),
                diameter: 4.0,
                flutes: 2,
                flute_length: 12.0,
                z_min: Some(8.0),
                operations: vec!["barrel".into()],
                ..SheetTool::default()
            },
        ],
        operations: vec![
            SheetOperation {
                name: format!("rough {name}"),
                kind: "indexed pocket".into(),
                tool: 1,
                spindle_rpm: 18000.0,
                feed: 800.0,
                plunge_feed: 200.0,
                rotary: "A90°".into(),
                cut_length: 1234.6,
                cut_minutes: 1.54,
                z_min: Some(-2.5),
                gouge_checked: true,
                ..SheetOperation::default()
            },
            SheetOperation {
                name: "barrel".into(),
                kind: "rotary_finish".into(),
                tool: 2,
                spindle_rpm: 24000.0,
                feed: 900.0,
                plunge_feed: 250.0,
                coolant: true,
                rotary: "A -180° to 180°".into(),
                cut_length: 3000.0,
                cut_minutes: 3.33,
                z_min: Some(8.0),
                gouge_checked: true,
            },
        ],
        totals: Totals {
            operations: 2,
            tools: 2,
            tool_changes: 2,
            cut_length: 4234.5,
            cut_minutes: 4.87,
            program_minutes: Some(4.9),
            program_lines: 1000,
        },
        extents: Some(ProgramExtents { min: [-20.0, -10.0, -2.5], max: [20.0, 10.0, 30.0], rotary: vec![("A".into(), -180.0, 180.0)] }),
        checks: vec![
            SheetCheck { name: "Read-back".into(), passed: true, detail: "within 0.004 mm".into() },
            SheetCheck { name: "Gouges".into(), passed: false, detail: format!("{name} cuts 1 mm in") },
        ],
        warnings: vec![format!("{name} is not checked")],
        notes: vec!["tessellated".into()],
        ..SetupSheet::default()
    }
}

#[test]
fn the_html_page_is_self_contained_and_escaped() {
    let nasty = "<script>alert(1)</script><img src=x onerror=alert(2)>";
    let s = sample(nasty);
    let h = s.html();
    assert!(h.starts_with("<!DOCTYPE html>") && h.ends_with("</html>\n"));
    assert!(!h.contains("<script") && !h.contains("<img") && !h.contains("http://") && !h.contains("https://"), "{h}");
    assert!(h.contains("&lt;script&gt;alert(1)&lt;/script&gt;"), "{h}");
    assert!(h.contains("@media print"), "print styles");
    for want in
        ["Work zero", "A axis", "Tools", "Operations", "Checks", "Warnings", "T1", "T2", "Ø34 x 4 + Ø34 x 18", "A90°", "FAILED", "18000", "G54"]
    {
        assert!(h.contains(want), "{want}: {h}");
    }
    assert!(!h.contains("Stock"), "no stock section without a stock");
    // The same sheet is the same bytes.
    assert_eq!(h, sample(nasty).html());
    let with_stock = SetupSheet { stock: Some("bar Ø25 x 60 <6061>".into()), ..sample("p") };
    assert!(with_stock.html().contains("bar Ø25 x 60 &lt;6061&gt;"));
}

/// A job written one program per tool: the header names those programs, not the one file of
/// every tool, which is not written (or is an old one, not to be run).
#[test]
fn a_split_sheet_names_its_programs_in_the_header() {
    let split = SetupSheet { program_file: None, program_files: vec!["part-1-T1.tap".into(), "part-2-T2.tap".into()], ..sample("p") };
    let h = split.html();
    assert!(h.contains("<tr><td>Programs</td><td>part-1-T1.tap, part-2-T2.tap</td></tr>"), "{h}");
    assert!(!h.contains("<td>Program</td>") && !h.contains("part.tap"), "{h}");
    let m = split.markdown();
    assert!(m.contains("- **Programs:** part-1-T1.tap, part-2-T2.tap\n") && !m.contains("**Program:**"), "{m}");
    // One program: as before.
    assert!(sample("p").html().contains("<tr><td>Program</td><td>part.tap</td></tr>"));
}

#[test]
fn the_markdown_sheet_keeps_its_tables_whole() {
    let s = sample("a | b\nc");
    let m = s.markdown();
    assert!(m.starts_with("# Setup sheet: a \\| b c\n"), "{m}");
    // Every table row has as many cells as its header.
    let tools = m.lines().filter(|l| l.strip_prefix("| T").is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))).collect::<Vec<_>>();
    assert_eq!(tools.len(), 2, "{m}");
    for row in m.lines().filter(|l| l.starts_with('|')) {
        let cells = row.replace("\\|", "").matches('|').count();
        assert!(cells == 12 || cells == 13 || cells == 4, "{row}");
    }
    assert!(m.contains("| 1 | rough a \\| b c | indexed pocket | T1 | 18000 | 800 | 200 | off | A90° | -2.5 | 1235 | 1.5 |"), "{m}");
    assert_eq!(m, sample("a | b\nc").render(SheetFormat::Markdown));
    assert_eq!(s.render(SheetFormat::Html), s.html());
}

fn side_op(name: &str, a: f64) -> MachineOperation {
    let moves = vec![
        Move::Rapid { to: Vec3::new(0.0, 0.0, 20.0) },
        Move::Rapid { to: Vec3::new(5.0, 5.0, 12.0) },
        Move::Feed { to: Vec3::new(5.0, 5.0, 10.0), feed: 200.0 },
        Move::Feed { to: Vec3::new(15.0, 5.0, 10.0), feed: 600.0 },
        Move::Rapid { to: Vec3::new(15.0, 5.0, 20.0) },
    ];
    MachineOperation::Indexed(IndexedOperation {
        name: name.into(),
        tool: Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0),
        spindle_rpm: 10000.0,
        coolant: false,
        rotary: vec![a],
        path: Toolpath { moves },
    })
}

/// A cut round the X axis at radius 10 from A0 to A270, the tool pointing away from the axis.
fn wrap_op() -> MachineOperation {
    let at = |deg: f64, r: f64| {
        let t = deg.to_radians();
        (Vec3::new(2.0, r * t.sin(), r * t.cos()), Vec3::new(0.0, t.sin(), t.cos()))
    };
    let mut poses = vec![at(0.0, 30.0)];
    poses.extend((0..=27).map(|k| at(f64::from(k) * 10.0, 10.0)));
    poses.push(at(270.0, 30.0));
    MachineOperation::Continuous(Operation5 {
        name: "wrap".into(),
        tool: Tool::new(2, ToolKind::BallEndmill, 4.0, 12.0),
        spindle_rpm: 12000.0,
        coolant: false,
        path: path(&poses),
        plunge_feed: None,
    })
}

fn named(ops: &[MachineOperation]) -> Vec<(&str, &Tool)> {
    ops.iter()
        .map(|o| match o {
            MachineOperation::Indexed(o) => (o.name.as_str(), &o.tool),
            MachineOperation::Continuous(o) => (o.name.as_str(), &o.tool),
        })
        .collect()
}

#[test]
fn an_indexed_operation_reaches_the_angle_it_asks_for() {
    let a = Machine::four_axis_a();
    let MachineOperation::Indexed(op) = side_op("s", -FRAC_PI_2) else { panic!("indexed") };
    let poses = op.poses(&a, 0.01).unwrap();
    let r = indexed_reach(&a, &op).unwrap().unwrap();
    assert_eq!(r.rotary, vec![(-FRAC_PI_2, -FRAC_PI_2)]);
    assert!((r.z_min - 10.0).abs() < 1e-12, "{r:?}");
    assert_eq!(r.last.map(|j| j.rotary), Some(vec![-FRAC_PI_2]));
    // From A90 the toolpath's nearest turn is A270, but the program writes A-90: so does this.
    let from = reach(&a, &poses, Some(&Joints { linear: Vec3::ZERO, rotary: vec![FRAC_PI_2] })).unwrap().unwrap();
    assert!((from.rotary[0].0 - 3.0 * FRAC_PI_2).abs() < 1e-9, "{from:?}");
    // Hostile operations are errors, not panics.
    let mut bad = op.clone();
    bad.rotary = vec![f64::NAN];
    assert!(indexed_reach(&a, &bad).is_err());
    bad.rotary = vec![];
    assert!(indexed_reach(&a, &bad).is_err());
    let mut bad = op.clone();
    bad.path.moves = vec![Move::Feed { to: Vec3::new(f64::INFINITY, 0.0, 0.0), feed: 100.0 }];
    assert!(indexed_reach(&a, &bad).is_err());
    bad.path.moves.clear();
    assert!(indexed_reach(&a, &bad).unwrap().is_none());
}

#[test]
fn each_operation_reaches_what_the_written_program_says() {
    let a = Machine::four_axis_a();
    let post = Post5::linuxcnc_4axis();
    // Two operations of the same name at A90 and A-90, then a cut round the bar.
    let ops = vec![side_op("side", FRAC_PI_2), side_op("side", -FRAC_PI_2), wrap_op()];
    let program = MachineProgram { name: "side".into(), operations: ops.clone(), work_offset: None };
    let text = emit_program(&program, &post, &a).unwrap();
    assert!(text.contains("A-90"), "{text}");
    let r = program_reach(&text, &post, &a, &named(&ops)).unwrap().unwrap();
    let [Some(r0), Some(r1), Some(r2)] = r.as_slice() else { panic!("{r:?}") };
    assert!((r0.rotary[0].0 - FRAC_PI_2).abs() < 1e-9 && (r0.rotary[0].1 - FRAC_PI_2).abs() < 1e-9, "{r0:?}");
    assert!((r1.rotary[0].0 + FRAC_PI_2).abs() < 1e-9 && (r1.rotary[0].1 + FRAC_PI_2).abs() < 1e-9, "{r1:?}");
    assert!((r0.z_min - 10.0).abs() < 1e-9 && (r1.z_min - 10.0).abs() < 1e-9, "{r:?}");
    // The cut round the bar: 270° of A, at the bar's radius. Its poses are 10° apart and the
    // post splits each move along its chord, so the written radius dips to 10 cos 5° between them.
    let (lo, hi) = r2.rotary[0];
    assert!(((hi - lo).to_degrees() - 270.0).abs() < 1e-3, "{r2:?}");
    assert!(r2.z_min <= 10.0 + 1e-9 && r2.z_min >= 10.0 * 5f64.to_radians().cos() - 1e-3, "{r2:?}");
    // Every range is within what the whole program turns through at feed.
    let angles: Vec<f64> = parse5(&text, &a.letters())
        .unwrap()
        .iter()
        .filter_map(|b| match b {
            JointWords::Feed { rotary, .. } => rotary.first().copied(),
            _ => None,
        })
        .collect();
    let (plo, phi) = angles.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| (l.min(*v), h.max(*v)));
    assert!(r.iter().flatten().all(|x| x.rotary[0].0 >= plo - 1e-12 && x.rotary[0].1 <= phi + 1e-12), "{r:?}");

    // A post without comments: the operations cannot be told apart.
    let mut quiet = post.clone();
    quiet.base.comments = CommentStyle::None;
    let text_q = emit_program(&program, &quiet, &a).unwrap();
    assert!(program_reach(&text_q, &quiet, &a, &named(&ops)).unwrap().is_none());
    // Hostile text and mismatched operations are no panic.
    assert!(program_reach("", &post, &a, &named(&ops)).unwrap().is_none());
    let one = named(&ops[..1]);
    let head = one.first().map(|(n, t)| format!("({})", post.base.comment_text(&crate::gcode::operation_comment(&post.base, n, t)))).unwrap();
    for junk in ["G1 X1e400 Y0 Z0 A0 F100", "\u{0}\u{7f}((\nG0 X0 Y0 Z5 A0", "G0 X0 Y0 Z1e308 A1e308\nG1 Z-1e308 F1", "G0 X0 Y0 Z5 A0\nG4 P1"] {
        let _ = program_reach(&format!("{head}\n{junk}\n"), &post, &a, &one);
    }
    let r = program_reach(&format!("{head}\nG0 X0 Y0 Z5 A0\nG4 P1\n"), &post, &a, &one).unwrap().unwrap();
    assert!(matches!(r.as_slice(), [Some(x)] if x.z_min == 5.0 && x.rotary == vec![(0.0, 0.0)]), "{r:?}");
    assert_eq!(program_reach(&text, &post, &a, &[]).unwrap(), Some(Vec::new()));
}

/// The stock is put in words for the operator: a block by its size and corners, a bar by its
/// diameter and ends.
#[test]
fn the_stock_is_described_by_its_size_and_place() {
    let block = crate::stock::Stock::Box { min: Vec3::new(-25.0, -20.0, -11.0), max: Vec3::new(25.0, 20.0, 0.5) };
    let w = stock_words(&block);
    assert!(w.starts_with("A block 50 x 40 x 11.5 mm"), "{w}");
    assert!(w.contains("from X-25 Y-20 Z-11 to X25 Y20 Z0.5") && w.contains("top is at Z0.5"), "{w}");
    let bar = crate::stock::Stock::Cylinder { radius: 12.5, x_min: -40.0, x_max: 35.0 };
    let w = stock_words(&bar);
    assert_eq!(w, "A bar 25 mm in diameter about the X axis (Y0 Z0), from X-40 to X35 (75 mm long).");
}

/// The fixtures are listed, and the machine's spindle-speed and feed limits the job runs into
/// come first, before the work zero, in both formats.
#[test]
fn fixtures_are_listed_and_the_machines_limits_come_first() {
    let s = SetupSheet {
        fixtures: vec!["`Vise jaw` X -40 to 40, Y -30 to -20, Z -20 to 0 <steel>".into()],
        attention: vec!["operation 0 `drill`: tool 160's 3700 rev/min is held to the spindle's range 10000 to 24000: 10000 rev/min".into()],
        ..sample("p")
    };
    let h = s.html();
    let first = h.find("Before you start").unwrap();
    assert!(first < h.find("Work zero").unwrap(), "{h}");
    assert!(h[first..].contains("held to the spindle"), "{h}");
    assert!(h.contains("<h2>Fixtures</h2>") && h.contains("`Vise jaw` X -40 to 40") && h.contains("&lt;steel&gt;"), "{h}");
    let m = s.markdown();
    let first = m.find("## Before you start").unwrap();
    assert!(first < m.find("## Work zero").unwrap(), "{m}");
    assert!(m.contains("## Fixtures") && m.contains("Vise jaw"), "{m}");
    // Without either, neither section.
    let plain = sample("p");
    assert!(!plain.html().contains("Before you start") && !plain.html().contains("Fixtures"));
    assert!(!plain.markdown().contains("Before you start") && !plain.markdown().contains("## Fixtures"));
}

/// base64 as RFC 4648 gives it, padding and all.
#[test]
fn base64_is_the_standard_alphabet_with_padding() {
    for (raw, enc) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
    {
        assert_eq!(base64(raw.as_bytes()), enc, "{raw}");
    }
    assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
}

/// The workholding, what to do at a tool change, the programs in order and the setup picture
/// are on the sheet in both formats when the job has them, and not otherwise.
#[test]
fn the_workholding_tool_changes_programs_and_picture_are_on_the_sheet() {
    let s = SetupSheet {
        workholding: Some("precision vise 175 <on the holes> (vise)".into()),
        tool_change: Some("This machine ignores tool changes (M6). Run the programs below in order.".into()),
        programs: vec!["part-1-T1.tap: T1, stickout 25 mm: rough (1.5 min at feed)".into(), "part-2-T2.tap: T2: barrel (3.3 min at feed)".into()],
        picture: Some(vec![0x89, b'P', b'N', b'G']),
        picture_file: Some("part setup.png".into()),
        ..sample("p")
    };
    let h = s.html();
    assert!(h.contains("<h2>Workholding</h2>\n<p>precision vise 175 &lt;on the holes&gt; (vise)</p>"), "{h}");
    assert!(h.contains("<h2>Tool changes</h2>") && h.find("Tool changes").unwrap() < h.find("Work zero").unwrap(), "{h}");
    let first = h.find("part-1-T1.tap").unwrap();
    assert!(first < h.find("part-2-T2.tap").unwrap() && h.contains("<ol>"), "in order: {h}");
    assert!(h.contains("href=\"data:image/png;base64,iVBORw==\"") && !h.contains("<img"), "{h}");
    // A real PNG's size is the picture's.
    let png = [&b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR"[..], &800u32.to_be_bytes(), &520u32.to_be_bytes()].concat();
    assert_eq!(png_size(&png), Some((800, 520)));
    assert_eq!(png_size(b"GIF89a"), None);
    let m = s.markdown();
    assert!(m.contains("## Workholding") && m.contains("## Tool changes"), "{m}");
    assert!(m.contains("1. [ ] part-1-T1.tap") && m.contains("2. [ ] part-2-T2.tap"), "{m}");
    assert!(m.contains("![The stock, the part and work zero](part%20setup.png)"), "{m}");
    let plain = sample("p");
    for h in [plain.html(), plain.markdown()] {
        assert!(!h.contains("Workholding") && !h.contains("Tool changes") && !h.contains("Programs, in the order") && !h.contains("base64"), "{h}");
    }
}

/// The probing steps come first on the sheet (before every other section), escaped, in both
/// formats, and the same bytes each time; the revision is in the header.
#[test]
fn the_probing_section_renders_first_escaped_and_the_same_twice() {
    // A 60 x 40 x 20 stock, its top at Z 0, 15.87 above the jaws, a 2 mm tip.
    let mut probing = probing_steps(Vec3::new(-30.0, -20.0, -20.0), Vec3::new(30.0, 20.0, 0.0), 15.87, 2.0);
    assert_eq!(probing.iter().map(|p| p.step).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6]);
    assert!(probing[1].touch.contains("X-35 Y0 Z-15.87") && probing[1].enter.contains("top - jaw_top + 4.13"), "{:?}", probing[1]);
    assert!(probing[2].touch.contains("3 mm below the top (Z-3)"), "{:?}", probing[2]);
    probing[0].enter.push_str(" <b>&");
    let s = SetupSheet {
        probing,
        revision: Some("R2-<x>".into()),
        stops: vec!["program 1 line 12: tool check T3 <gap 10>".into()],
        attention: vec!["limits".into()],
        ..sample("p")
    };
    let h = s.html();
    assert_eq!(h, s.html());
    let first = h.find("<h2>").unwrap();
    assert_eq!(h.get(first..first + 16), Some("<h2>Probing</h2>"), "{h}");
    assert!(h.contains("&lt;b&gt;&amp;") && !h.contains("<b>&") && h.contains("R2-&lt;x&gt;") && h.contains("&lt;gap 10&gt;"), "{h}");
    let m = s.markdown();
    assert_eq!(m, s.markdown());
    let first = m.find("\n## ").unwrap();
    assert_eq!(m.get(first..first + 12), Some("\n## Probing\n"), "{m}");
    assert!(m.contains("1. Touch the stock's top") && m.contains("6. Touch nothing: Cycle Start") && !m.contains("<b>"), "{m}");
    assert!(m.contains("- **Revision:**") && m.contains("## Stops"), "{m}");
    // A sheet without probing has none of it.
    let plain = sample("p");
    for t in [plain.html(), plain.markdown()] {
        assert!(!t.contains("Probing") && !t.contains("Revision") && !t.contains("Stops"), "{t}");
    }
}
