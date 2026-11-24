//! Excellon reader tests. The drill files are written by hand to look like what common PCB
//! tools export; expected values are worked out by hand from the format rules.

use super::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn at(p: Vec2, x: f64, y: f64) -> bool {
    close(p.x, x) && close(p.y, y)
}

fn read(text: &str) -> Drills {
    match read_excellon(text) {
        Ok(d) => d,
        Err(e) => panic!("the drill file should read: {e}"),
    }
}

fn refusal(text: &str) -> String {
    match read_excellon(text) {
        Ok(d) => panic!("the drill file should be refused, got {d:?}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn metric_decimal_file_with_attributes() {
    let d = read(
        "M48\n\
         ; DRILL file {test writer} date 2026-01-01\n\
         ; FORMAT={-:-/ absolute / metric / decimal}\n\
         ; #@! TF.FileFunction,Plated,1,2,PTH\n\
         FMAT,2\n\
         METRIC\n\
         ; #@! TA.AperFunction,Plated,PTH,ComponentDrill\n\
         T1C0.800\n\
         ; #@! TA.AperFunction,Plated,PTH,ViaDrill\n\
         T2C1.000\n\
         %\n\
         G90\n\
         G05\n\
         T1\n\
         X10.0Y-5.0\n\
         X12.54Y-5.0\n\
         T2\n\
         X20Y30.5\n\
         T0\n\
         M30\n",
    );
    assert_eq!(d.hits.len(), 3);
    assert!(d.slots.is_empty());
    assert!(at(d.hits[0].at, 10.0, -5.0) && close(d.hits[0].diameter, 0.8));
    assert!(at(d.hits[1].at, 12.54, -5.0));
    // A decimal file writes a whole number without its `.0`.
    assert!(at(d.hits[2].at, 20.0, 30.5) && close(d.hits[2].diameter, 1.0));
    assert!(d.hits.iter().all(|h| h.plated == Some(true)));
    // The hole function says which are vias.
    assert_eq!(d.hits.iter().map(|h| h.via).collect::<Vec<_>>(), [Some(false), Some(false), Some(true)]);
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
}

/// A header that lists its plated tools under `;TYPE=PLATED` and its unplated ones under
/// `;TYPE=NON_PLATED`, as Altium writes it, gives each tool its own plating.
#[test]
fn each_type_comment_plates_the_tools_after_it() {
    let d = read(
        "M48
INCH,LZ
;TYPE=PLATED
T1C0.00984
;TYPE=NON_PLATED
T2C0.12795
%
T01
X001000Y001000
T02
X002000Y001000
M30
",
    );
    assert_eq!(d.hits.iter().map(|h| h.plated).collect::<Vec<_>>(), [Some(true), Some(false)]);
}

#[test]
fn inch_two_four_with_leading_zeros_kept() {
    // LZ: leading zeros are written, so digits count from the left (2 integer digits).
    let d = read(
        "M48\n\
         ;Layer_Color=9474304\n\
         ;FILE_FORMAT=2:4\n\
         INCH,LZ\n\
         ;TYPE=PLATED\n\
         T1F00S00C0.0280\n\
         T2F00S00C0.0400\n\
         %\n\
         T01\n\
         X011500Y-0175\n\
         X02Y003\n\
         T02\n\
         X0105Y01\n\
         M30\n",
    );
    assert_eq!(d.hits.len(), 3);
    assert!(at(d.hits[0].at, 1.15 * 25.4, -1.75 * 25.4));
    assert!(at(d.hits[1].at, 2.0 * 25.4, 0.3 * 25.4));
    assert!(at(d.hits[2].at, 1.05 * 25.4, 25.4));
    assert!(close(d.hits[0].diameter, 0.028 * 25.4));
    assert!(close(d.hits[2].diameter, 0.04 * 25.4));
    assert!(d.hits.iter().all(|h| h.plated == Some(true)));
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
}

#[test]
fn inch_with_trailing_zeros_kept_uses_the_default_two_four() {
    // TZ: trailing zeros are written, so digits count from the right (4 decimal digits).
    let d = read("M48\nINCH,TZ\nT1C0.035\n%\nT1\nX11500Y-17500\nX500Y5\nM30\n");
    assert_eq!(d.hits.len(), 2);
    assert!(at(d.hits[0].at, 1.15 * 25.4, -1.75 * 25.4));
    assert!(at(d.hits[1].at, 0.05 * 25.4, 0.0005 * 25.4));
    assert!(close(d.hits[0].diameter, 0.035 * 25.4));
}

#[test]
fn metric_implied_digits_default_and_from_the_header() {
    let d = read("M48\nMETRIC\nT1C1.0\n%\nT1\nX12500Y-3000\nM30\n");
    assert!(at(d.hits[0].at, 12.5, -3.0));
    let d = read("M48\nMETRIC,LZ,000.00\nT1C1.0\n%\nT1\nX01234Y005\nM30\n");
    assert!(at(d.hits[0].at, 12.34, 5.0));
    let d = read("M48\n;FORMAT={3:3/ absolute / metric / suppress leading zeros}\nMETRIC\nT1C1.0\n%\nT1\nX1500Y-20\nM30\n");
    assert!(at(d.hits[0].at, 1.5, -0.02));
}

#[test]
fn a_missing_axis_keeps_its_last_value() {
    let d = read("M48\nMETRIC\nT1C1.0\n%\nT1\nX1.0Y2.0\nX3.0\nY4.0\nM30\n");
    let pts: Vec<_> = d.hits.iter().map(|h| (h.at.x, h.at.y)).collect();
    assert_eq!(pts, vec![(1.0, 2.0), (3.0, 2.0), (3.0, 4.0)]);
}

#[test]
fn incremental_positions_add_to_the_last_one() {
    let d = read("M48\nMETRIC\nT1C1.0\n%\nG91\nT1\nX1.0Y1.0\nX1.0Y1.0\nY-0.5\nG90\nX7.0\nM30\n");
    let pts: Vec<_> = d.hits.iter().map(|h| (h.at.x, h.at.y)).collect();
    assert_eq!(pts, vec![(1.0, 1.0), (2.0, 2.0), (2.0, 1.5), (7.0, 1.5)]);
    let d = read("M48\nMETRIC\nICI,ON\nT1C1.0\n%\nT1\nX1.0Y1.0\nX1.0\nM30\n");
    assert!(at(d.hits[1].at, 2.0, 1.0));
}

#[test]
fn g85_slot_goes_from_the_first_position_to_the_second() {
    let d = read("M48\nMETRIC\nT1C1.0\n%\nT1\nX1.0Y2.0G85X5.0\nY3.0\nM30\n");
    assert_eq!(d.slots.len(), 1);
    let s = &d.slots[0];
    assert!(at(s.a, 1.0, 2.0) && at(s.b, 5.0, 2.0) && close(s.diameter, 1.0));
    // The tool ends at the slot's end.
    assert_eq!(d.hits.len(), 1);
    assert!(at(d.hits[0].at, 5.0, 3.0));
}

#[test]
fn rout_mode_cuts_slots_only_with_the_tool_down() {
    let d = read(
        "M48\nMETRIC\nT1C0.6\n%\nG05\nT1\nX0.5Y0.5\n\
         G00X1.0Y1.0\nM15\nG01X1.0Y4.0\nX2.0\nM16\nG01X8.0Y8.0\n\
         G05\nX9.0Y9.0\nM30\n",
    );
    assert_eq!(d.slots.len(), 2);
    assert!(at(d.slots[0].a, 1.0, 1.0) && at(d.slots[0].b, 1.0, 4.0) && close(d.slots[0].diameter, 0.6));
    assert!(at(d.slots[1].a, 1.0, 4.0) && at(d.slots[1].b, 2.0, 4.0));
    let pts: Vec<_> = d.hits.iter().map(|h| (h.at.x, h.at.y)).collect();
    assert_eq!(pts, vec![(0.5, 0.5), (9.0, 9.0)]);
}

fn arc_is(arc: &DrillArc, c: (f64, f64), a: (f64, f64), b: (f64, f64)) -> bool {
    at(arc.c, c.0, c.1) && at(arc.a, a.0, a.1) && at(arc.b, b.0, b.1)
}

/// One routed cut from (10, 0) with a 1 mm tool.
fn routed(cut: &str) -> Drills {
    read(&format!("M48\nMETRIC\nT1C1.0\n%\nT1\nG00X10.0Y0.0\nM15\n{cut}\nM16\nG05\nM30\n"))
}

#[test]
fn rout_arcs_with_centre_offsets_are_arc_slots() {
    // A quarter turn round the origin, counter-clockwise.
    let d = routed("G03X0.0Y10.0I-10.0J0.0");
    assert!(d.slots.is_empty());
    assert_eq!(d.arcs.len(), 1);
    assert!(arc_is(&d.arcs[0], (0.0, 0.0), (10.0, 0.0), (0.0, 10.0)) && close(d.arcs[0].diameter, 1.0));
    // The same quarter clockwise, from (0, 10): stored with its ends swapped.
    let d = read("M48\nMETRIC\nT1C1.0\n%\nT1\nG00X0.0Y10.0\nM15\nG02X10.0Y0.0I0.0J-10.0\nM16\nM30\n");
    assert!(arc_is(&d.arcs[0], (0.0, 0.0), (10.0, 0.0), (0.0, 10.0)));
    // Back to the start: a full circle.
    let d = routed("G03X10.0Y0.0I-10.0");
    assert!(arc_is(&d.arcs[0], (0.0, 0.0), (10.0, 0.0), (10.0, 0.0)));
    assert_eq!(d.to_geom(), vec![Geom2::Circle(Vec2::new(0.0, 0.0), 10.5), Geom2::Circle(Vec2::new(0.0, 0.0), 9.5)]);
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
}

#[test]
fn rout_arcs_with_a_radius_take_the_shorter_or_longer_way() {
    // Chord from (10, 0) to (0, 10); radius 10 puts the centre at (0, 0) or (10, 10).
    let ccw_short = routed("G03X0.0Y10.0A10.0");
    assert!(arc_is(&ccw_short.arcs[0], (0.0, 0.0), (10.0, 0.0), (0.0, 10.0)), "{:?}", ccw_short.arcs);
    let cw_short = routed("G02X0.0Y10.0A10.0");
    assert!(arc_is(&cw_short.arcs[0], (10.0, 10.0), (0.0, 10.0), (10.0, 0.0)), "{:?}", cw_short.arcs);
    let ccw_long = routed("G03X0.0Y10.0A-10.0");
    assert!(arc_is(&ccw_long.arcs[0], (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)), "{:?}", ccw_long.arcs);
}

#[test]
fn degenerate_rout_arcs_are_skipped_with_a_warning() {
    // A radius too small to reach, no centre at all, and a turn tighter than the tool; the
    // tool still ends where each cut ends.
    let d = read(
        "M48\nMETRIC\nT1C1.0\n%\nT1\nG00X10.0Y0.0\nM15\n\
         G03X0.0Y10.0A1.0\nG02X0.0Y0.0\nG03X0.4Y0.0I0.2\nG01X5.0Y0.0\nM16\nM30\n",
    );
    assert!(d.arcs.is_empty());
    assert!(d.warnings.iter().any(|w| w.contains("3 routed arc(s)")), "{:?}", d.warnings);
    assert_eq!(d.slots.len(), 1);
    assert!(at(d.slots[0].a, 0.4, 0.0) && at(d.slots[0].b, 5.0, 0.0));
}

#[test]
fn repeat_codes_step_from_the_last_hole() {
    let d = read("M48\nMETRIC\nT1C1.0\n%\nT1\nX1.0Y1.0\nR3X2.0\nR2Y-0.5\nR0X9.0\nX20.0\nM30\n");
    let pts: Vec<_> = d.hits.iter().map(|h| (h.at.x, h.at.y)).collect();
    assert_eq!(pts, vec![(1.0, 1.0), (3.0, 1.0), (5.0, 1.0), (7.0, 1.0), (7.0, 0.5), (7.0, 0.0), (20.0, 0.0)]);
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    // A huge count stops at the limit, and costs nothing when there is no tool.
    let d = read("M48\nMETRIC\nT1C1.0\n%\nT1\nX0.0Y0.0\nR4294967295X0.001\nM30\n");
    assert_eq!(d.hits.len(), MAX_ENTITIES);
    assert!(d.warnings.iter().any(|w| w.contains("more than")), "{:?}", d.warnings);
    let d = read("M48\nMETRIC\nT1C1.0\n%\nX0.0Y0.0\nR4294967295X0.001\nM30\n");
    assert!(d.hits.is_empty());
    assert!(d.warnings.iter().any(|w| w.contains("4294967296 hole(s)")), "{:?}", d.warnings);
}

#[test]
fn coordinates_read_with_guessed_decimal_places_say_so() {
    let d = read(
        "M48
METRIC
T1C1.0
%
T1
X1100000Y600000
M30
",
    );
    assert_eq!(d.guessed_decimals, Some(3));
    assert!(at(d.hits[0].at, 1100.0, 600.0));
    let d = read(
        "M48
METRIC,TZ,000.0000
T1C1.0
%
T1
X1100000Y600000
M30
",
    );
    assert_eq!(d.guessed_decimals, None);
    let d = read(
        "M48
METRIC
T1C1.0
%
T1
X110.0Y60.0
M30
",
    );
    assert_eq!(d.guessed_decimals, None, "decimal points need no guess");
}

#[test]
fn an_overlong_digit_pattern_falls_back_to_the_usual_digits() {
    let d = read(&format!("M48\nMETRIC,LZ,{}\nT1C1.0\n%\nT1\nX012500Y-0030\nM30\n", "0".repeat(200)));
    assert!(at(d.hits[0].at, 12.5, -3.0));
    assert!(d.warnings.iter().any(|w| w.contains("number format")), "{:?}", d.warnings);
}

#[test]
fn plating_comes_from_file_and_tool_attributes() {
    let mixed = read(
        "M48\n; #@! TF.FileFunction,MixedPlating,1,2\nMETRIC\n\
         ; #@! TA.AperFunction,Plated,PTH,ComponentDrill\nT1C1.0\n\
         ; #@! TA.AperFunction,NonPlated,NPTH,ComponentDrill\nT2C3.0\n%\n\
         T1\nX1.0Y1.0\nT2\nX2.0Y2.0G85X4.0Y2.0\nM30\n",
    );
    assert_eq!(mixed.hits[0].plated, Some(true));
    assert_eq!(mixed.slots[0].plated, Some(false));
    let npth = read("M48\n; #@! TF.FileFunction,NonPlated,1,2,NPTH\nMETRIC\nT1C3.0\n%\nT1\nX1.0Y1.0\nM30\n");
    assert_eq!(npth.hits[0].plated, Some(false));
    let typed = read(";TYPE=NON_PLATED\nM48\nMETRIC,LZ,0000.00000\nT01C2.54000\n%\nG05\nG90\nT01\nX168.529Y-73.152\nM30\n");
    assert_eq!(typed.hits[0].plated, Some(false));
    assert!(close(typed.hits[0].diameter, 2.54));
    let unknown = read("M48\nMETRIC\nT1C1.0\n%\nT1\nX1.0Y1.0\nM30\n");
    assert_eq!(unknown.hits[0].plated, None);
}

#[test]
fn holes_of_an_undefined_tool_are_skipped_with_a_warning() {
    let d = read("M48\nMETRIC\nT1C1.0\n%\nT3\nX1.0Y1.0\nX2.0Y2.0\nT1\nX5.0Y5.0\nM30\n");
    assert_eq!(d.hits.len(), 1);
    assert!(at(d.hits[0].at, 5.0, 5.0));
    assert!(d.warnings.iter().any(|w| w.contains("2 hole(s)") && w.contains("T3")), "{:?}", d.warnings);
    // Nothing usable: an empty result that says why.
    let d = read("M48\nMETRIC\nT1C1.0\n%\nX1.0Y1.0\nM30\n");
    assert!(d.hits.is_empty());
    assert!(d.warnings[0].contains("no holes or slots"), "{:?}", d.warnings);
    assert!(d.warnings.iter().any(|w| w.contains("before any tool")), "{:?}", d.warnings);
}

#[test]
fn absurd_sizes_and_positions_are_skipped() {
    let d = read("M48\nMETRIC\nT1C0\nT2C1000\nT3C1.0\n%\nT1\nX1.0Y1.0\nT2\nX2.0Y2.0\nT3\nX9999999.0Y0.0\nX3.0Y3.0\nM30\n");
    assert_eq!(d.hits.len(), 1);
    assert!(at(d.hits[0].at, 3.0, 3.0));
    assert!(d.warnings.iter().any(|w| w.contains("T1") && w.contains("unusable")), "{:?}", d.warnings);
    assert!(d.warnings.iter().any(|w| w.contains("T2") && w.contains("refused")), "{:?}", d.warnings);
    assert!(d.warnings.iter().any(|w| w.contains("from the origin")), "{:?}", d.warnings);
}

#[test]
fn unknown_commands_warn_and_m30_ends_the_file() {
    let d = read("M48\nMETRIC\nFROB,1\nT1C1.0\n%\nT1\nG93X0Y0\nX1.0Y1.0\nM30\nX2.0Y2.0\n");
    assert_eq!(d.hits.len(), 1);
    assert!(d.warnings.iter().any(|w| w.contains("FROB")), "{:?}", d.warnings);
    assert!(d.warnings.iter().any(|w| w.contains("G93")), "{:?}", d.warnings);
}

#[test]
fn a_header_only_drill_file_is_empty_and_other_text_is_refused() {
    // As written for a board without unplated holes.
    let empty = "M48\n; DRILL file {test writer}\n; FORMAT={-:-/ absolute / metric / decimal}\n\
                 ; #@! TF.FileFunction,NonPlated,1,2,NPTH\nFMAT,2\nMETRIC\n%\nG90\nG05\nM30\n";
    for text in [empty, "M48\nMETRIC\nT1C1.0\n%\nM30\n", "M48\n"] {
        let d = read(text);
        assert!(d.hits.is_empty() && d.slots.is_empty() && d.arcs.is_empty());
        assert_eq!(d.warnings, vec!["the drill file has no holes or slots".to_string()]);
    }
    for text in ["", "\n\n", "%", "hello world\n"] {
        assert!(refusal(text).contains("not an Excellon drill file"));
    }
    let big = "X".repeat(MAX_DRAWING_BYTES + 1);
    assert!(refusal(&big).contains("too large"));
}

#[test]
fn hostile_input_never_panics() {
    // Fixed cases first, then pseudo-random bytes and pseudo-random Excellon-like lines.
    let cases = [
        "M48\nMETRIC\nT1C1.0\n%\nT1\nX99999999999999999999999999999Y1\n",
        "M48\nMETRIC\nT1C1.0\n%\nT1\nX1e400Y1\n",
        "M48\nMETRIC\nT1C1.0\n%\nT1\nX1.2.3Y-\n",
        "M48\nMETRIC\nT1C.\n%\nT1\nX1Y1\n",
        "M48\nMETRIC\nT4294967296C1\n%\nT99999999999\nX1Y1\n",
        "M48\nMETRIC,LZ,000000000000.0000000000\nT1C1\n%\nT1\nX1Y1\n",
        "X1Y1G85G85G85\nG85\nX--1Y++1\nT\nC\n;\n;#@!\n;FORMAT={\n;FORMAT={9:9\n",
        "\u{feff}M48\r\nINCH,TZ\r\nT1C0.01\r\n%\r\nT1\r\nX1Y1\r\nM30\r\n",
        "M48\nMETRIC\nT1C1.0\n%\nT1\nG91\nX900000.0\nX900000.0\nX900000.0\n",
        "M48\nMETRIC\nT1C1.0\n%\nT1\nX0Y0\nR4294967295X999999\nR99999999999X1\nRX1\nR-1\nR1Z1\n",
        "M48\nMETRIC\nT1C1.0\n%\nT1\nG00X0Y0\nM15\nG02X1Y1I1e9\nG03X1Y1I999999999J999999999\nG03X5Y5A0\nG02X5Y5\nG03X1Y1A-0.0001\n",
    ];
    for c in cases {
        let _ = read_excellon(c).map(|d| d.to_geom());
    }
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let alphabet = b"XYTCGMFSRIJA0123456789.-+%;,\n LZTZINCHMETRIC#@!";
    for _ in 0..300 {
        let len = (next() % 400) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| (next() & 0xff) as u8).collect();
        let _ = read_excellon(&String::from_utf8_lossy(&bytes)).map(|d| d.to_geom());
        let text: String = (0..len).map(|_| alphabet[(next() % alphabet.len() as u64) as usize] as char).collect();
        let _ = read_excellon(&format!("M48\nMETRIC\nT1C1.0\nT2C0.5\n%\nT1\n{text}")).map(|d| d.to_geom());
    }
}

#[test]
fn hits_are_capped() {
    let mut text = String::from("M48\nMETRIC\nT1C1.0\n%\nT1\n");
    for i in 0..=MAX_ENTITIES {
        text.push_str(&format!("X{}.0Y0.0\n", i % 1000));
    }
    let d = read(&text);
    assert_eq!(d.hits.len(), MAX_ENTITIES);
    assert!(d.warnings.iter().any(|w| w.contains("more than")), "{:?}", d.warnings);
}

fn ends(g: &Geom2) -> (Vec2, Vec2) {
    match g {
        Geom2::Line(a, b) => (*a, *b),
        Geom2::Arc { a, b, .. } => (*a, *b),
        other => panic!("a slot outline holds lines and arcs, got {other:?}"),
    }
}

#[test]
fn slot_outline_is_one_closed_loop_with_shared_end_points() {
    for (a, b) in [((1.0, 2.0), (5.0, 2.0)), ((-3.3, 0.7), (4.1, -2.9)), ((0.0, 0.0), (0.0, -7.0))] {
        let drills =
            Drills { slots: vec![DrillSlot { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1), diameter: 1.0, plated: None }], ..Drills::default() };
        let g = drills.to_geom();
        assert_eq!(g.len(), 4);
        for i in 0..4 {
            assert_eq!(ends(&g[i]).1, ends(&g[(i + 1) % 4]).0, "piece {i} of {g:?}");
        }
        // Each half circle has radius 0.5 and bulges away from the slot (counter-clockwise).
        for (arc, centre, away) in [(&g[1], b, (b.0 - a.0, b.1 - a.1)), (&g[3], a, (a.0 - b.0, a.1 - b.1))] {
            let Geom2::Arc { c, a: p, b: q } = arc else { panic!("expected an arc, got {arc:?}") };
            assert!(at(*c, centre.0, centre.1));
            let (rp, rq) = ((p.x - c.x).hypot(p.y - c.y), (q.x - c.x).hypot(q.y - c.y));
            assert!(close(rp, 0.5) && close(rq, 0.5));
            // Turning counter-clockwise from p, the quarter point lies along `away`.
            let mid = (-(p.y - c.y), p.x - c.x);
            assert!(mid.0 * away.0 + mid.1 * away.1 > 0.0, "arc {arc:?}");
        }
    }
    let first = Drills { hits: vec![DrillHit { at: Vec2::new(1.0, 1.0), diameter: 0.8, plated: None, via: None }], ..Drills::default() };
    assert_eq!(first.to_geom(), vec![Geom2::Circle(Vec2::new(1.0, 1.0), 0.4)]);
    let dot = Drills { slots: vec![DrillSlot { a: Vec2::new(2.0, 3.0), b: Vec2::new(2.0, 3.0), diameter: 1.2, plated: None }], ..Drills::default() };
    assert_eq!(dot.to_geom(), vec![Geom2::Circle(Vec2::new(2.0, 3.0), 0.6)]);
}

#[test]
fn arc_slot_outline_is_one_closed_loop_of_exact_arcs() {
    // Quarter turn of radius 10 round the origin, 1 mm tool.
    let arc = DrillArc { c: Vec2::new(0.0, 0.0), a: Vec2::new(10.0, 0.0), b: Vec2::new(0.0, 10.0), diameter: 1.0, plated: None };
    let g = arc.outline();
    let want = [
        ((0.0, 0.0), (10.5, 0.0), (0.0, 10.5)),
        ((0.0, 10.0), (0.0, 10.5), (0.0, 9.5)),
        ((0.0, 0.0), (9.5, 0.0), (0.0, 9.5)),
        ((10.0, 0.0), (9.5, 0.0), (10.5, 0.0)),
    ];
    assert_eq!(g.len(), 4);
    for (piece, (c, a, b)) in g.iter().zip(want) {
        let Geom2::Arc { c: pc, a: pa, b: pb } = piece else { panic!("expected an arc, got {piece:?}") };
        assert!(at(*pc, c.0, c.1) && at(*pa, a.0, a.1) && at(*pb, b.0, b.1), "{piece:?}");
    }
    // Every end point is shared by exactly two pieces: one closed loop.
    let ends: Vec<Vec2> = g.iter().flat_map(|p| [ends(p).0, ends(p).1]).collect();
    for p in &ends {
        assert_eq!(ends.iter().filter(|q| **q == *p).count(), 2, "{g:?}");
    }
    let drills = Drills { arcs: vec![arc.clone()], ..Drills::default() };
    assert_eq!(drills.to_geom(), g);
    // Tighter than its tool: no outline.
    assert!(DrillArc { diameter: 25.0, ..arc }.outline().is_empty());
}

#[test]
fn contributors_board_plated_drill_file() {
    // The plated drill file of a contributor's own board, exported unchanged.
    let d = read(include_str!("../tests/fixtures/pcb/treasure_hunt_pth.drl"));
    assert_eq!(d.hits.len(), 90);
    assert_eq!(d.slots.len(), 2);
    assert!(at(d.hits[0].at, 136.525, -93.345) && close(d.hits[0].diameter, 0.305));
    let s = &d.slots[0];
    assert!(at(s.a, 133.75, -95.4629) && at(s.b, 132.95, -95.4629) && close(s.diameter, 0.8));
    assert!(d.hits.iter().all(|h| h.plated == Some(true)) && d.slots.iter().all(|s| s.plated == Some(true)));
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    assert_eq!(d.to_geom().len(), 90 + 2 * 4);
}
