//! Gerber outline and image layer reader tests. Every fixture is written by hand here, except the contributor's
//! own board outline in `tests/fixtures/pcb/`; expected values are worked out by hand.

use super::*;

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn read(text: &str) -> GerberOutline {
    match read_gerber_outline(text) {
        Ok(o) => o,
        Err(e) => panic!("expected an outline, got: {e}"),
    }
}

fn refusal(text: &str) -> String {
    match read_gerber_outline(text) {
        Ok(o) => panic!("expected a refusal, got {o:?}"),
        Err(e) => e.to_string(),
    }
}

fn has_warning(o: &GerberOutline, needle: &str) -> bool {
    o.warnings.iter().any(|w| w.contains(needle))
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

/// The end points of an entity, in no particular order.
fn ends(g: &Geom2) -> Vec<Vec2> {
    match g {
        Geom2::Line(a, b) | Geom2::Arc { a, b, .. } => vec![*a, *b],
        _ => Vec::new(),
    }
}

/// Every entity shares an end point, bit for bit, with the next one (and the last with the first).
fn shares_exact_ends(geom: &[Geom2]) -> bool {
    (0..geom.len()).all(|k| {
        let here = ends(&geom[k]);
        let next = ends(&geom[(k + 1) % geom.len()]);
        here.iter().any(|a| next.iter().any(|b| a.x.to_bits() == b.x.to_bits() && a.y.to_bits() == b.y.to_bits()))
    })
}

/// A 30 x 20 mm board with 2 mm corner radii, drawn counter-clockwise the way PCB tools write
/// an edge-cuts layer (format 4.6, millimetres, multi-quadrant arcs).
const ROUNDED_CCW: &str = "\
G04 Board outline, hand-written test fixture*
%TF.FileFunction,Profile,NP*%
%FSLAX46Y46*%
G04 Gerber Fmt 4.6, Leading zero omitted, Abs format (unit mm)*
%MOMM*%
%LPD*%
G01*
G04 APERTURE LIST*
%ADD10C,0.100000*%
G04 APERTURE END LIST*
D10*
X2000000Y0D02*
X28000000D01*
G75*
G03X30000000Y2000000I0J2000000D01*
G01*
X30000000Y18000000D01*
G03X28000000Y20000000I-2000000J0D01*
G01*
X2000000Y20000000D01*
G03X0Y18000000I0J-2000000D01*
G01*
X0Y2000000D01*
G03X2000000Y0I2000000J0D01*
M02*
";

#[test]
fn a_rounded_rectangle_outline_reads_as_lines_and_counter_clockwise_arcs() {
    let o = read(ROUNDED_CCW);
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(
        o.geom,
        vec![
            Geom2::Line(p(2.0, 0.0), p(28.0, 0.0)),
            Geom2::Arc { c: p(28.0, 2.0), a: p(28.0, 0.0), b: p(30.0, 2.0) },
            Geom2::Line(p(30.0, 2.0), p(30.0, 18.0)),
            Geom2::Arc { c: p(28.0, 18.0), a: p(30.0, 18.0), b: p(28.0, 20.0) },
            Geom2::Line(p(28.0, 20.0), p(2.0, 20.0)),
            Geom2::Arc { c: p(2.0, 18.0), a: p(2.0, 20.0), b: p(0.0, 18.0) },
            Geom2::Line(p(0.0, 18.0), p(0.0, 2.0)),
            Geom2::Arc { c: p(2.0, 2.0), a: p(0.0, 2.0), b: p(2.0, 0.0) },
        ]
    );
    assert!(shares_exact_ends(&o.geom));
}

#[test]
fn clockwise_arcs_swap_their_ends_to_run_counter_clockwise() {
    // The same board drawn clockwise from (0, 2), all on one line per corner.
    let text = "%FSLAX46Y46*%%MOMM*%%ADD10C,0.1*%D10*G75*\
        X0Y2000000D02*G01X0Y18000000D01*G02X2000000Y20000000I2000000J0D01*\
        G01X28000000Y20000000D01*G02X30000000Y18000000I0J-2000000D01*\
        G01X30000000Y2000000D01*G02X28000000Y0I-2000000J0D01*\
        G01X2000000Y0D01*G02X0Y2000000I0J2000000D01*M02*";
    let o = read(text);
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(
        o.geom,
        vec![
            Geom2::Line(p(0.0, 2.0), p(0.0, 18.0)),
            Geom2::Arc { c: p(2.0, 18.0), a: p(2.0, 20.0), b: p(0.0, 18.0) },
            Geom2::Line(p(2.0, 20.0), p(28.0, 20.0)),
            Geom2::Arc { c: p(28.0, 18.0), a: p(30.0, 18.0), b: p(28.0, 20.0) },
            Geom2::Line(p(30.0, 18.0), p(30.0, 2.0)),
            Geom2::Arc { c: p(28.0, 2.0), a: p(28.0, 0.0), b: p(30.0, 2.0) },
            Geom2::Line(p(28.0, 0.0), p(2.0, 0.0)),
            Geom2::Arc { c: p(2.0, 2.0), a: p(0.0, 2.0), b: p(2.0, 0.0) },
        ]
    );
    assert!(shares_exact_ends(&o.geom));
}

#[test]
fn an_inch_file_is_read_in_millimetres() {
    // A 1 x 0.5 inch board, format 2.4: 10000 is one inch.
    let text = "%FSLAX24Y24*%\n%MOIN*%\n%ADD10C,0.004*%\nD10*\nG01*\n\
        X0Y0D02*\nX10000Y0D01*\nX10000Y5000D01*\nX0Y5000D01*\nX0Y0D01*\nM02*\n";
    let o = read(text);
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    let corners = [p(0.0, 0.0), p(25.4, 0.0), p(25.4, 12.7), p(0.0, 12.7)];
    assert_eq!(o.geom.len(), 4);
    for (k, g) in o.geom.iter().enumerate() {
        let Geom2::Line(a, b) = g else { panic!("expected a line, got {g:?}") };
        assert!(close(*a, corners[k]) && close(*b, corners[(k + 1) % 4]), "{g:?}");
    }
    assert!(shares_exact_ends(&o.geom));
}

#[test]
fn coordinates_left_out_keep_their_previous_value() {
    let text = "%FSLAX46Y46*%%MOMM*%D10*G01*X0Y0D02*X10000000D01*Y5000000D01*X0D01*Y0D01*M02*";
    let o = read(text);
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(
        o.geom,
        vec![
            Geom2::Line(p(0.0, 0.0), p(10.0, 0.0)),
            Geom2::Line(p(10.0, 0.0), p(10.0, 5.0)),
            Geom2::Line(p(10.0, 5.0), p(0.0, 5.0)),
            Geom2::Line(p(0.0, 5.0), p(0.0, 0.0)),
        ]
    );
    // A block without its D code reuses the last one (deprecated): read, with a warning.
    let o = read("%FSLAX46Y46*%%MOMM*%G01*X0Y0D02*X1000000Y0D01*X1000000Y1000000*M02*");
    assert_eq!(o.geom.len(), 2);
    assert_eq!(o.geom[1], Geom2::Line(p(1.0, 0.0), p(1.0, 1.0)));
    assert!(has_warning(&o, "previous one"), "{:?}", o.warnings);
}

#[test]
fn an_arc_back_to_its_start_is_a_full_circle() {
    // A round board of radius 5 mm around the origin.
    let o = read("%FSLAX46Y46*%%MOMM*%D10*G75*X5000000Y0D02*G03X5000000Y0I-5000000J0D01*M02*");
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(o.geom, vec![Geom2::Circle(p(0.0, 0.0), 5.0)]);
}

#[test]
fn an_outline_drawn_as_a_region_gives_its_boundary() {
    let text = "%FSLAX46Y46*%%MOMM*%G01*G36*X0Y0D02*X10000000Y0D01*X10000000Y10000000D01*\
        X0Y10000000D01*X0Y0D01*G37*M02*";
    let o = read(text);
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(
        o.geom,
        vec![
            Geom2::Line(p(0.0, 0.0), p(10.0, 0.0)),
            Geom2::Line(p(10.0, 0.0), p(10.0, 10.0)),
            Geom2::Line(p(10.0, 10.0), p(0.0, 10.0)),
            Geom2::Line(p(0.0, 10.0), p(0.0, 0.0)),
        ]
    );
    // A contour that does not come back to its start is read, with a warning.
    let o = read("%FSLAX46Y46*%%MOMM*%G01*G36*X0Y0D02*X10000000Y0D01*X10000000Y10000000D01*G37*M02*");
    assert_eq!(o.geom.len(), 2);
    assert!(has_warning(&o, "does not end where it starts"), "{:?}", o.warnings);
}

#[test]
fn single_quadrant_arcs_find_their_centre_from_unsigned_offsets() {
    // Clockwise quarter from (0, 10) to (10, 0) around the origin: the true offset is J = -10,
    // written unsigned.
    let o = read("%FSLAX46Y46*%%MOMM*%G74*X0Y10000000D02*G02X10000000Y0I0J10000000D01*M02*");
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(o.geom, vec![Geom2::Arc { c: p(0.0, 0.0), a: p(10.0, 0.0), b: p(0.0, 10.0) }]);
    // Counter-clockwise quarter of radius 4 around (1, 1) from (5, 1) to (1, 5): I = -4 unsigned.
    let o = read("%FSLAX46Y46*%%MOMM*%G74*X5000000Y1000000D02*G03X1000000Y5000000I4000000J0D01*M02*");
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(o.geom, vec![Geom2::Arc { c: p(1.0, 1.0), a: p(5.0, 1.0), b: p(1.0, 5.0) }]);
}

#[test]
fn a_layer_that_is_not_an_outline_is_read_with_a_warning() {
    let text = "%TF.FileFunction,Copper,L1,Top*%%FSLAX46Y46*%%MOMM*%%ADD10C,0.25*%D10*G01*\
        X0Y0D02*X5000000Y0D01*X1000000Y1000000D03*M02*";
    let o = read(text);
    assert_eq!(o.geom, vec![Geom2::Line(p(0.0, 0.0), p(5.0, 0.0))]);
    assert!(has_warning(&o, "`Copper,L1,Top` layer, not a board outline"), "{:?}", o.warnings);
    assert!(has_warning(&o, "Flashed apertures"), "{:?}", o.warnings);
    // The attribute may also come as a comment.
    let o = read("G04 #@! TF.FileFunction,Legend,Top*%FSLAX46Y46*%%MOMM*%G01*X0Y0D02*X5000000Y0D01*M02*");
    assert!(has_warning(&o, "`Legend,Top` layer"), "{:?}", o.warnings);
    // Pads only: refused, saying why.
    let msg = refusal("%FSLAX46Y46*%%MOMM*%%ADD10C,1*%D10*X0Y0D03*X5000000Y0D03*M02*");
    assert!(msg.contains("no lines or arcs") && msg.contains("pads"), "{msg}");
}

#[test]
fn deprecated_coordinate_formats_are_read_with_a_warning() {
    // Trailing zeros omitted, format 2.4 inch: `01` is 01.0000 inch.
    let o = read("%FSTAX24Y24*%%MOIN*%G01*X0Y0D02*X01D01*M02*");
    assert_eq!(o.geom.len(), 1);
    let Geom2::Line(a, b) = o.geom[0] else { panic!("expected a line") };
    assert!(close(a, p(0.0, 0.0)) && close(b, p(25.4, 0.0)), "{:?}", o.geom);
    assert!(has_warning(&o, "trailing zeros"), "{:?}", o.warnings);
    // Incremental coordinates add to the current point.
    let o = read("%FSLIX46Y46*%%MOMM*%G01*X1000000Y0D02*X1000000Y0D01*M02*");
    assert_eq!(o.geom, vec![Geom2::Line(p(1.0, 0.0), p(2.0, 0.0))]);
    assert!(has_warning(&o, "incremental"), "{:?}", o.warnings);
}

#[test]
fn the_contributors_own_board_outline_reads_as_a_closed_octagon() {
    let o = read(include_str!("../tests/fixtures/pcb/treasure_hunt_outline.gko"));
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(o.geom.len(), 8);
    assert!(o.geom.iter().all(|g| matches!(g, Geom2::Line(..))));
    // Format 4.5 millimetres: X13443288 is 134.43288 mm.
    let Geom2::Line(a, b) = o.geom[0] else { panic!("expected a line") };
    assert!(close(a, p(134.43288, -100.584)) && close(b, p(131.191, -97.13008)), "{:?}", o.geom[0]);
    assert!(shares_exact_ends(&o.geom));
    let xs: Vec<f64> = o.geom.iter().flat_map(ends).map(|v| v.x).collect();
    let ys: Vec<f64> = o.geom.iter().flat_map(ends).map(|v| v.y).collect();
    let span = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min);
    assert!((span(&xs) - 81.534).abs() < 1e-9 && (span(&ys) - 50.292).abs() < 1e-9);
}

#[test]
fn unreadable_files_are_refused_with_a_reason() {
    assert!(refusal("").contains("no lines or arcs"));
    assert!(refusal("G04 only a comment*\nM02*").contains("no lines or arcs"));
    assert!(refusal("G01*X0Y0D02*X1Y1D01*M02*").contains("format statement"));
    assert!(refusal("%FSLAX46Y46*%%MOFT*%").contains("unknown unit"));
    assert!(refusal("%FSQQX46Y46*%").contains("cannot be read"));
    // Coordinates past a kilometre or with absurd digit counts.
    assert!(refusal("%FSLAX66Y66*%%MOIN*%G01*X0Y0D02*X999999999999D01*M02*").contains("kilometre"));
    assert!(refusal("%FSLAX46Y46*%%MOMM*%X99999999999999999999999Y1D02*M02*").contains("digits"));
    // More than 6 decimals, X and Y in different formats, incremental mode by G91.
    assert!(refusal("%FSLAX17Y17*%%MOMM*%X1Y1D02*M02*").contains("at most 6 decimals"));
    assert!(refusal("%FSLAX46Y36*%").contains("different numbers of digits"));
    assert!(refusal("%FSLAX46Y46*%G91*").contains("G91"));
    let big = " ".repeat(MAX_DRAWING_BYTES + 1);
    assert!(refusal(&big).contains("too large"));
}

#[test]
fn a_zero_radius_arc_is_drawn_as_a_line() {
    let o = read("%FSLAX46Y46*%%MOMM*%G75*X0Y0D02*G03X1000000Y0I0J0D01*M02*");
    assert_eq!(o.geom, vec![Geom2::Line(p(0.0, 0.0), p(1.0, 0.0))]);
    assert!(has_warning(&o, "zero radius"), "{:?}", o.warnings);
}

#[test]
fn broken_and_unknown_commands_are_warnings() {
    // An unterminated `%` at the end, an unknown command and a missing end.
    let o = read("%FSLAX46Y46*%%MOMM*%G01*X0Y0D02*X1000000Y0D01*Q123*%ZZ1*%%MOMM");
    assert_eq!(o.geom.len(), 1);
    assert!(has_warning(&o, "inside a `%` command"), "{:?}", o.warnings);
    assert!(has_warning(&o, "`Q123`"), "{:?}", o.warnings);
    assert!(has_warning(&o, "`%ZZ1%`"), "{:?}", o.warnings);
    assert!(has_warning(&o, "no end command"), "{:?}", o.warnings);
}

#[test]
fn every_command_reaches_the_parser_on_its_own_line() {
    // Several commands per line, and one command split over lines, read as one per line.
    let one_line = "%FSLAX46Y46*%%MOMM*%%ADD10C,0.1*%D10*G01*X0Y0D02*X1000000Y0D01*X1000000Y1000000D01*M02*";
    let split = "%FSLAX4\n6Y46*%\n%MOMM*%G01*X0Y0\nD02*X1000000Y0D01*X10000\r\n00Y1000000D01*M02*";
    let expected = vec![Geom2::Line(p(0.0, 0.0), p(1.0, 0.0)), Geom2::Line(p(1.0, 0.0), p(1.0, 1.0))];
    for text in [one_line, split] {
        let o = read(text);
        assert!(o.warnings.is_empty(), "{:?}", o.warnings);
        assert_eq!(o.geom, expected);
    }
    // Several extended commands in one `%...%`.
    let o = read("%FSLAX46Y46*MOMM*%G01*X0Y0D02*X1000000Y0D01*M02*");
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(o.geom, vec![Geom2::Line(p(0.0, 0.0), p(1.0, 0.0))]);
    // An arc given only by its offsets keeps the current point's X and Y.
    let o = read("%FSLAX46Y46*%%MOMM*%G75*X5000000Y0D02*G03I-5000000J0D01*M02*");
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(o.geom, vec![Geom2::Circle(p(0.0, 0.0), 5.0)]);
    // Deprecated G70 units and the G55 flash prefix.
    let o = read("%FSLAX24Y24*%G70*G01*X0Y0D02*X10000Y0D01*G55D03*M02*");
    let Geom2::Line(_, b) = o.geom[0] else { panic!("expected a line") };
    assert!(close(b, p(25.4, 0.0)), "{:?}", o.geom);
    // The unit said twice, as `%MOIN*%` and G70, is no warning.
    let o = read("%FSLAX24Y24*%%MOIN*%G70*G01*X0Y0D02*X10000Y0D01*M02*");
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
}

#[test]
fn lines_the_parser_would_slice_out_of_bounds_are_warnings() {
    // `gerber_parser` 0.5 slices `G0...` lines at byte 3: a short one, or one with a non-ASCII
    // character there, would panic inside it.
    assert!(!safe_for_parser("G0"));
    assert!(!safe_for_parser("G0*"));
    assert!(!safe_for_parser("G0\u{e9}*"));
    assert!(safe_for_parser("G01*"));
    let draw = "%FSLAX46Y46*%%MOMM*%G01*X0Y0D02*X1000000Y0D01*";
    // `G0` then a line break is one block with what follows: `G0 M02`, an unknown G code.
    let o = read(&format!("{draw}G0\nM02*"));
    assert_eq!(o.geom.len(), 1);
    assert!(has_warning(&o, "unknown G code G00"), "{:?}", o.warnings);
    // At the very end it has no `*`.
    let o = read(&format!("{draw}G0\n"));
    assert_eq!(o.geom.len(), 1);
    assert!(has_warning(&o, "missing its closing `*`"), "{:?}", o.warnings);
    let o = read(&format!("{draw}G0\u{e9}*M02*"));
    assert_eq!(o.geom.len(), 1);
    assert!(has_warning(&o, "not plain ASCII"), "{:?}", o.warnings);
    // File-function attributes the parser would index past their end are read by the pre-pass.
    let o = read(&format!("%TF.FileFunction,Legend*%G04 #@! TF.FileFunction,Soldermask*{draw}M02*"));
    assert!(has_warning(&o, "`Soldermask` layer"), "{:?}", o.warnings);
}

/// A small deterministic generator, so a failure always reproduces.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
        from.get((self.next() % from.len().max(1) as u64) as usize).copied().unwrap_or("")
    }
}

/// Inputs aimed at every panic site known in `gerber_parser` 0.5.0 (file:line in its
/// `src/parser.rs` / `src/util.rs`); the pre-pass keeps each one from reaching it (see the
/// module comment of `gerber.rs`).
const PARSER_PANIC_PROBES: &[&str] = &[
    // parser.rs:222 `&line[3..]` on `G0...` lines.
    "G0\n",
    "G0*",
    "G0\u{e9}*",
    "G0\u{e9}",
    "G1*",
    "G01X\u{e9}*",
    // parser.rs:514 image mirroring, 696 an operation's D code, 840 comments, 1604 a lone D code.
    "%MIA1B1*%",
    "%MIA2B0*%",
    "%MIB*%",
    "X1Y1D4*",
    "X1Y1D0*",
    "X1D*",
    "G04 #@! *",
    "G04#@! TF.X*",
    "G4 c*",
    "D1*",
    "D4*",
    "D0*",
    "D00*",
    // parser.rs:2408, 2437, 2467 macro values.
    "%ADD10OC8,1.5X$1X1+2*%",
    "%ADD10M,X*%",
    "%AMX*4,1,3,0,0,1,1,2,2,0*%",
    "%AMX*0 c*$1=1*21,1,$1,1,0,0,0*%",
    // parser.rs:2077 `split_first_str` unwrap, and attribute argument indexing.
    "%TF*%",
    "%TF.FileFunction,Legend*%",
    "%TA*%",
    "%TO*%",
    "G04 #@! TF*",
    "G04 #@! TA.AperFunction*",
    // util.rs: `6u8 - fs.decimal` and `10i64.pow(..)` overflow.
    "%FSLAX17Y17*%X1Y1D02*",
    "%FSLAX60Y60*%X9999999999999999999Y1D02*",
    "%FSTAX66Y66*%X999999999999999D02*",
    // Broken structure, huge numbers and unknown extended commands.
    "%",
    "%%%%",
    "*****",
    "%FSLAX46Y46",
    "%FSLAX99Y99*%%MOIN*%G03X-999999999999999999Y999999999999999999I1J1D01*",
    "%FSLAX46Y46*%G74*G02X1Y1I0J0D01*G02X0Y0D01*",
    "%FSLAX46Y46*%%MOMM*%G36*G36*G37*G37*D03*D99*M99*G999*X-Y+D01*",
    "%ABD10*%%AB*%%SRX3Y3I1J1*%%IR90*%%LMQ*%%LR*%%OFA*%%SFB*%%IPX*%%ASX*%%LPX*%",
    "G04 \u{1f600}*X\u{1f600}Y1D01*",
];

/// Commands and fragments the generated files are made of.
const FRAGMENTS: &[&str] = &[
    "%FSLAX46Y46*%",
    "%FSTAX24Y24*%",
    "%FSLIX36Y36*%",
    "%MOMM*%",
    "%MOIN*%",
    "G01*",
    "G02*",
    "G03*",
    "G74*",
    "G75*",
    "G36*",
    "G37*",
    "D10*",
    "D01*",
    "D02*",
    "D03*",
    "%ADD10C,0.1*%",
    "%ADD11R,1X2*%",
    "%ADD14M*%",
    "%LPC*%",
    "%SRX2Y2I1J1*%",
    "%AMOC8*5,1,8,0,0,1.08239X$1,22.5*%",
    "%AMRR*21,1,$1,$2,0,0,$3*1,0,($1-$2)x2,0,0*4,1,3,0,0,1,0,1,1,0,0,0*7,0,0,2,1,0.3,45*$4=$1/0*%",
    "%ADD12RR,1X0.5X30*%",
    "%ADD13RR*%",
    "%ADD15O,1X2X0.3*%",
    "%ADD16P,2X6X15*%",
    "%LPD*%",
    "M02*",
    "*",
    "%",
    "\n",
    "\u{e9}",
];

/// The reader never panics: on the probes (alone, after a valid header, and on their own line),
/// on random command sequences with random numbers, and on mutations of a real board outline.
#[test]
fn hostile_input_never_panics() {
    let header = "%FSLAX46Y46*%%MOMM*%%ADD10C,0.1*%D10*G01*X0Y0D02*X1000000Y0D01*";
    for probe in PARSER_PANIC_PROBES {
        for text in [probe.to_string(), format!("{header}{probe}M02*"), format!("{header}\n{probe}\nX0Y0D01*M02*")] {
            let _ = read_gerber_outline(&text);
            let _ = read_gerber_layer(&text);
        }
    }
    let mut rng = XorShift(0x9e37_79b9_7f4a_7c15);
    let number = |rng: &mut XorShift| {
        let digits: String = (0..1 + rng.next() % 14).map(|_| char::from(b'0' + (rng.next() % 10) as u8)).collect();
        if rng.next().is_multiple_of(3) { format!("-{digits}") } else { digits }
    };
    for _ in 0..500 {
        let mut text = String::new();
        for _ in 0..40 {
            if rng.next().is_multiple_of(3) {
                for letter in ["X", "Y", "I", "J"] {
                    if rng.next().is_multiple_of(2) {
                        text.push_str(letter);
                        text.push_str(&number(&mut rng));
                    }
                }
                text.push_str(rng.pick(&["D01*", "D02*", "D03*", "*"]));
            } else {
                text.push_str(rng.pick(FRAGMENTS));
                if rng.next().is_multiple_of(8) {
                    text.push_str(rng.pick(PARSER_PANIC_PROBES));
                }
            }
        }
        let _ = read_gerber_outline(&text);
        let _ = read_gerber_layer(&text);
    }
    let fixture: Vec<char> = include_str!("../tests/fixtures/pcb/treasure_hunt_outline.gko").chars().collect();
    for _ in 0..500 {
        let mut text = fixture.clone();
        for _ in 0..1 + rng.next() % 6 {
            let at = (rng.next() % text.len().max(1) as u64) as usize;
            let piece: Vec<char> = match rng.next() % 3 {
                0 => vec![char::from(b' ' + (rng.next() % 95) as u8)],
                1 => rng.pick(PARSER_PANIC_PROBES).chars().collect(),
                _ => text.get(at..(at + 1 + (rng.next() % 40) as usize).min(text.len())).unwrap_or_default().to_vec(),
            };
            if rng.next().is_multiple_of(2) && at < text.len() {
                text.remove(at);
            }
            let at = at.min(text.len());
            text.splice(at..at, piece);
        }
        let text: String = text.into_iter().collect();
        let _ = read_gerber_outline(&text);
        let _ = read_gerber_layer(&text);
    }
}

// ---- image layers ----

const LAYER_HEAD: &str = "%FSLAX46Y46*%%MOMM*%G75*";

fn layer(body: &str) -> GerberLayer {
    match read_gerber_layer(&format!("{LAYER_HEAD}{body}M02*")) {
        Ok(l) => l,
        Err(e) => panic!("expected a layer, got: {e}"),
    }
}

/// A closed contour's signed area, arcs exact (counter-clockwise positive); its pieces run in
/// order, each either way.
fn loop_area(l: &[Geom2]) -> f64 {
    if let [Geom2::Circle(_, r)] = l {
        return std::f64::consts::PI * r * r;
    }
    let mut area = 0.0;
    let mut at: Option<Vec2> = None;
    for g in l {
        let (a, b, bulge_area) = match *g {
            Geom2::Line(a, b) => (a, b, 0.0),
            Geom2::Arc { c, a, b } => {
                let r = (a.x - c.x).hypot(a.y - c.y);
                let sweep = ((b.y - c.y).atan2(b.x - c.x) - (a.y - c.y).atan2(a.x - c.x)).rem_euclid(std::f64::consts::TAU);
                (a, b, r * r / 2.0 * (sweep - sweep.sin()))
            }
            _ => continue,
        };
        // Pieces run on from the previous end; a piece the other way round counts backwards.
        let forward = at.is_none_or(|p| close(p, a));
        let (a, b, seg) = if forward { (a, b, bulge_area) } else { (b, a, -bulge_area) };
        area += (a.x * b.y - b.x * a.y) / 2.0 + seg;
        at = Some(b);
    }
    area
}

/// The covered area: outer contours less holes (nesting by size is enough for these fixtures).
fn covered(l: &GerberLayer) -> f64 {
    let mut areas: Vec<f64> = l.loops.iter().map(|x| loop_area(x).abs()).collect();
    areas.sort_by(|a, b| b.total_cmp(a));
    areas.iter().enumerate().map(|(i, a)| if i == 0 { *a } else { -a }).sum::<f64>().abs()
}

#[test]
fn overlapping_pads_join_into_one_area() {
    let l = layer("%ADD10R,2X2*%D10*X0Y0D03*X1000000Y0D03*");
    assert_eq!(l.loops.len(), 1, "{:?}", l.loops);
    assert!((covered(&l) - 6.0).abs() < 1e-9, "{}", covered(&l));
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn a_round_trace_is_a_stadium_with_exact_arcs() {
    let l = layer("%ADD10C,1*%D10*G01*X0Y0D02*X10000000Y0D01*");
    let arcs = l.loops.iter().flatten().filter(|g| matches!(g, Geom2::Arc { .. })).count();
    assert_eq!((l.loops.len(), arcs), (1, 2), "{:?}", l.loops);
    let want = 10.0 + std::f64::consts::PI * 0.25;
    assert!((covered(&l) - want).abs() < 1e-9, "{} vs {want}", covered(&l));
}

#[test]
fn clear_polarity_cuts_a_hole() {
    let l = layer("G36*X0Y0D02*G01*X10000000Y0D01*Y10000000D01*X0D01*Y0D01*G37*%LPC*%%ADD10C,2*%D10*X5000000Y5000000D03*");
    assert_eq!(l.loops.len(), 2, "{:?}", l.loops);
    assert!((covered(&l) - (100.0 - std::f64::consts::PI)).abs() < 1e-9, "{}", covered(&l));
}

#[test]
fn a_region_reaching_its_hole_by_a_cut_in_line_has_a_hole() {
    // Outer square, in to the inner square along y = 3, round it the other way, and back out.
    let l = layer(
        "G36*G01*X0Y0D02*X10000000D01*Y10000000D01*X0D01*Y3000000D01*X3000000D01*Y7000000D01*X7000000D01*Y3000000D01*X3000000D01*X0D01*Y0D01*G37*",
    );
    assert_eq!(l.loops.len(), 2, "{:?}", l.loops);
    assert!((covered(&l) - 84.0).abs() < 1e-9, "{}", covered(&l));
}

#[test]
fn an_aperture_macro_with_expressions_and_an_erasing_primitive() {
    // A 4 by 2 rectangle less a circle of diameter ($1 - $2) / 2 = 1 at its centre.
    let l = layer("%AMBOX*21,1,$1,$2,0,0,0*$3=($1-$2)/2*1,0,$3,0,0*%%ADD10BOX,4X2*%D10*X0Y0D03*");
    assert_eq!(l.loops.len(), 2, "{:?} {:?}", l.loops, l.warnings);
    assert!((covered(&l) - (8.0 - std::f64::consts::PI * 0.25)).abs() < 1e-9, "{}", covered(&l));
}

#[test]
fn a_circle_drawn_with_a_round_aperture_is_a_ring() {
    let l = layer("%ADD10C,0.2*%D10*X2000000Y0D02*G03*X2000000Y0I-2000000J0D01*");
    assert_eq!(l.loops.len(), 2, "{:?}", l.loops);
    let want = std::f64::consts::PI * (2.1f64.powi(2) - 1.9f64.powi(2));
    assert!((covered(&l) - want).abs() < 1e-9, "{} vs {want}", covered(&l));
}

#[test]
fn objects_without_an_aperture_are_skipped_with_a_warning() {
    let l = layer("D11*X0Y0D03*");
    assert!(l.loops.is_empty() && l.warnings.iter().any(|w| w.contains("never defines")), "{:?}", l.warnings);
}
