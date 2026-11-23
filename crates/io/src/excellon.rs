//! Excellon drill files (NC drill): the holes and routed slots of a printed circuit board, in
//! millimetres. Written from the published format description, not from another reader.
//!
//! What is read:
//! - the header (`M48` up to `%` or `M95`) with `METRIC` / `INCH`, `LZ` / `TZ`, an optional
//!   digit pattern (`METRIC,TZ,000.000`), `FMAT,2`, `ICI,ON|OFF`, and the format comments
//!   writers add (`;FORMAT={3:3/ absolute / metric / decimal}`, `;FILE_FORMAT=2:4`);
//! - tool definitions (`T1C0.800`, `T01F00S00C0.8`) and tool selection (`T1`, `T0` unloads);
//! - coordinates with a decimal point or as implied-decimal integers. `LZ` means leading zeros
//!   are kept (trailing ones may be dropped, so digits count from the left), `TZ` means
//!   trailing zeros are kept (digits count from the right, the default). Without a stated
//!   format, metric numbers are 3.3 digits and inch numbers 2.4. A file whose coordinates have
//!   decimal points (or whose format comment says `decimal`) reads a coordinate without one as
//!   a whole number (`X140` is 140), since such writers drop a trailing `.0`;
//! - modal coordinates (a missing X or Y keeps the last one), `G90` absolute and `G91`
//!   incremental positions (incremental values add to the last position);
//! - drill mode (`G05`), `G85` slots (`X..Y..G85X..Y..`) and rout mode (`G00` move, `M15`
//!   tool down, `G01` cuts, `M16`/`M17` tool up). Arcs in rout mode (`G02` clockwise, `G03`
//!   counter-clockwise) are kept as arc slots ([`DrillArc`]), their centre given by `I`/`J`
//!   offsets from the start or by a radius `A` (negative: the longer way round). An arc with no
//!   centre that reaches both ends, or tighter than its tool, is skipped with a warning;
//! - repeats (`R<n>X..Y..`): `n` more holes, each stepped by X and Y from the last one;
//! - plating from attribute comments (`#@! TF.FileFunction,Plated|NonPlated`,
//!   `#@! TA.AperFunction,Plated|NonPlated,...` before a tool) and `;TYPE=PLATED|NON_PLATED`;
//! - `M30` ends the program. Anything else is a warning, not an error. A drill file with no
//!   holes reads as empty with a warning; text with nothing of a drill file in it is refused.

use std::collections::{BTreeMap, BTreeSet};

use solidcore_record::Vec2;

use crate::sketch2d::{MAX_DRAWING_BYTES, MAX_ENTITIES};
use crate::{Geom2, IoError, Result};

/// One drilled hole.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillHit {
    /// Centre (mm, y up).
    pub at: Vec2,
    /// Tool diameter (mm).
    pub diameter: f64,
    /// Plated through, when the file says (`None`: not stated).
    pub plated: Option<bool>,
    /// A via (`Some(true)`) or a component or mechanical hole (`Some(false)`), when the file
    /// says with a `#@! TA.AperFunction` attribute (`None`: not stated).
    pub via: Option<bool>,
}

/// One routed slot: the tool moved from `a` to `b`, cutting a slot as wide as the tool.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillSlot {
    pub a: Vec2,
    pub b: Vec2,
    /// Tool diameter, the slot's width (mm).
    pub diameter: f64,
    pub plated: Option<bool>,
}

/// One slot routed along a circular arc: the tool's centre ran counter-clockwise around `c`
/// from `a` to `b` (a clockwise cut is stored with its ends swapped; the slot is the same).
/// `a == b` is a full circle.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillArc {
    /// Centre of the tool path (mm).
    pub c: Vec2,
    pub a: Vec2,
    pub b: Vec2,
    /// Tool diameter, the slot's width (mm).
    pub diameter: f64,
    pub plated: Option<bool>,
}

impl DrillArc {
    /// The slot's outline: the outer and inner arcs around `c` and a half circle round each
    /// end, sharing their end points exactly. A full circle is two concentric circles. Empty
    /// when the arc cannot be a slot (no radius, or tighter than the tool).
    pub fn outline(&self) -> Vec<Geom2> {
        let mut out = Vec::new();
        if usable_diameter(self.diameter) && finite(self.c) && finite(self.a) && finite(self.b) {
            arc_outline(self.c, self.a, self.b, self.diameter / 2.0, &mut out);
        }
        out
    }
}

/// What a drill file holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drills {
    pub hits: Vec<DrillHit>,
    pub slots: Vec<DrillSlot>,
    /// Slots routed along arcs (`G02`/`G03` in rout mode).
    pub arcs: Vec<DrillArc>,
    /// Things read but not understood, or guessed, worded for a person.
    pub warnings: Vec<String>,
    /// The decimal places coordinates written without a decimal point were read with, when the
    /// file doesn't say (its numbers are then a power of ten out if the guess is wrong).
    pub guessed_decimals: Option<usize>,
}

impl Drills {
    /// The holes as sketch geometry: a circle per hit, per slot its outline (two half circles
    /// joined by two lines), per routed arc its outline ([`DrillArc::outline`]).
    pub fn to_geom(&self) -> Vec<Geom2> {
        let pieces = self.slots.len().saturating_add(self.arcs.len()).saturating_mul(4);
        let mut out = Vec::with_capacity(self.hits.len().saturating_add(pieces));
        for h in &self.hits {
            if usable_diameter(h.diameter) && finite(h.at) {
                out.push(Geom2::Circle(h.at, h.diameter / 2.0));
            }
        }
        for s in &self.slots {
            if usable_diameter(s.diameter) && finite(s.a) && finite(s.b) {
                slot_outline(s.a, s.b, s.diameter / 2.0, &mut out);
            }
        }
        for a in &self.arcs {
            out.extend(a.outline());
        }
        out
    }
}

/// The outline of an arc slot of half width `r` along the counter-clockwise arc around `c`
/// from `a` to `b`: outer arc, end cap at `b`, inner arc, end cap at `a`.
fn arc_outline(c: Vec2, a: Vec2, b: Vec2, r: f64, out: &mut Vec<Geom2>) {
    let radius = (a.x - c.x).hypot(a.y - c.y);
    let rb = (b.x - c.x).hypot(b.y - c.y);
    if !(radius - r > 1e-9) || !(rb > 1e-9) {
        return;
    }
    let (inner, outer) = (radius - r, radius + r);
    if (a.x - b.x).hypot(a.y - b.y) < 1e-9 {
        out.push(Geom2::Circle(c, outer));
        out.push(Geom2::Circle(c, inner));
        return;
    }
    let (ua, ub) = (((a.x - c.x) / radius, (a.y - c.y) / radius), ((b.x - c.x) / rb, (b.y - c.y) / rb));
    let on = |u: (f64, f64), d: f64| Vec2::new(c.x + u.0 * d, c.y + u.1 * d);
    let (a_out, a_mid, a_in) = (on(ua, outer), on(ua, radius), on(ua, inner));
    let (b_out, b_mid, b_in) = (on(ub, outer), on(ub, radius), on(ub, inner));
    out.push(Geom2::Arc { c, a: a_out, b: b_out });
    out.push(Geom2::Arc { c: b_mid, a: b_out, b: b_in });
    out.push(Geom2::Arc { c, a: a_in, b: b_in });
    out.push(Geom2::Arc { c: a_mid, a: a_in, b: a_out });
}

fn usable_diameter(d: f64) -> bool {
    d.is_finite() && d > 0.0
}

fn finite(p: Vec2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// The closed outline of a slot of radius `r` from `a` to `b`: the right side, the half circle
/// around `b`, the left side, the half circle around `a`. Neighbouring pieces share their end
/// points exactly, so they join into one loop.
fn slot_outline(a: Vec2, b: Vec2, r: f64, out: &mut Vec<Geom2>) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = dx.hypot(dy);
    if !(len > 1e-9) {
        out.push(Geom2::Circle(a, r));
        return;
    }
    // Left normal (direction turned a quarter counter-clockwise), scaled to the radius.
    let (nx, ny) = (-dy / len * r, dx / len * r);
    let a_left = Vec2::new(a.x + nx, a.y + ny);
    let a_right = Vec2::new(a.x - nx, a.y - ny);
    let b_left = Vec2::new(b.x + nx, b.y + ny);
    let b_right = Vec2::new(b.x - nx, b.y - ny);
    out.push(Geom2::Line(a_right, b_right));
    out.push(Geom2::Arc { c: b, a: b_right, b: b_left });
    out.push(Geom2::Line(b_left, a_left));
    out.push(Geom2::Arc { c: a, a: a_left, b: a_right });
}

/// Largest tool diameter taken as real (mm); bigger ones are refused.
const MAX_DIAMETER_MM: f64 = 100.0;
/// Farthest position from the origin taken as real (mm).
const MAX_COORD_MM: f64 = 1e6;
/// Most warnings kept; the rest are counted.
const MAX_WARNINGS: usize = 100;
/// Most distinct unknown commands reported.
const MAX_UNKNOWN: usize = 64;
/// Most tool definitions in one file.
const MAX_TOOLS: usize = 10_000;
/// Longest number read (characters).
const MAX_NUMBER_CHARS: usize = 32;
const MM_PER_INCH: f64 = 25.4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Units {
    Mm,
    Inch,
}

/// How implied-decimal integers are written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zeros {
    /// `LZ`: leading zeros kept, trailing ones may be dropped; digits count from the left.
    KeepLeading,
    /// `TZ`: trailing zeros kept, leading ones may be dropped; digits count from the right.
    KeepTrailing,
}

#[derive(Clone, Copy)]
enum Tool {
    Ok {
        diameter: f64,
        plated: Option<bool>,
        via: Option<bool>,
    },
    /// Defined with a size we do not believe.
    Refused,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Move,
    Line,
    /// `G02`.
    Clockwise,
    /// `G03`.
    CounterClockwise,
}

struct Reader {
    units: Units,
    units_from_command: bool,
    zeros: Zeros,
    zeros_from_command: bool,
    /// Integer and decimal digits of implied-decimal numbers, when stated.
    digits: Option<(usize, usize)>,
    digits_from_command: bool,
    incremental: bool,
    /// The file writes coordinates with decimal points: one without is a whole number.
    decimal_points: bool,
    /// The decimal places guessed for a coordinate, when the file didn't say.
    guessed: std::cell::Cell<Option<usize>>,
    in_header: bool,
    rout: bool,
    motion: Motion,
    tool_down: bool,
    /// Current position (mm).
    pos: (f64, f64),
    tool: Option<u32>,
    tools: BTreeMap<u32, Tool>,
    file_plated: Option<bool>,
    pending_plated: Option<bool>,
    pending_via: Option<bool>,
    /// The plating a `;TYPE=` comment gives the tools defined after it.
    section_plated: Option<bool>,
    out: Drills,
    dropped_warnings: usize,
    unknown: BTreeSet<String>,
    no_tool: usize,
    undefined: BTreeMap<u32, usize>,
    refused: BTreeMap<u32, usize>,
    far: usize,
    bad_arcs: usize,
    too_many_tools_warned: bool,
    /// Something only a drill file has was seen (header, units, a tool, a hole).
    looks_like_drill: bool,
    stop: bool,
}

/// Read an Excellon drill file.
pub fn read_excellon(text: &str) -> Result<Drills> {
    if text.len() > MAX_DRAWING_BYTES {
        return Err(IoError::Invalid("the drill file is too large to read".into()));
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut r = Reader::new();
    r.decimal_points = writes_decimal_points(text);
    for line in text.split(['\n', '\r']) {
        r.line(line);
        if r.stop {
            break;
        }
    }
    r.finish()
}

impl Reader {
    fn new() -> Self {
        Self {
            units: Units::Mm,
            units_from_command: false,
            zeros: Zeros::KeepTrailing,
            zeros_from_command: false,
            digits: None,
            digits_from_command: false,
            incremental: false,
            decimal_points: false,
            guessed: std::cell::Cell::new(None),
            in_header: false,
            rout: false,
            motion: Motion::Move,
            tool_down: false,
            pos: (0.0, 0.0),
            tool: None,
            tools: BTreeMap::new(),
            file_plated: None,
            pending_plated: None,
            pending_via: None,
            section_plated: None,
            out: Drills::default(),
            dropped_warnings: 0,
            unknown: BTreeSet::new(),
            no_tool: 0,
            undefined: BTreeMap::new(),
            refused: BTreeMap::new(),
            far: 0,
            bad_arcs: 0,
            too_many_tools_warned: false,
            looks_like_drill: false,
            stop: false,
        }
    }

    fn warn(&mut self, message: String) {
        if self.out.warnings.len() < MAX_WARNINGS {
            self.out.warnings.push(message);
        } else {
            self.dropped_warnings = self.dropped_warnings.saturating_add(1);
        }
    }

    /// Report a command we do not understand, once per command.
    fn unknown(&mut self, what: &str) {
        let what: String = what.chars().take(24).collect();
        if self.unknown.len() < MAX_UNKNOWN && self.unknown.insert(what.clone()) {
            self.warn(format!("`{what}` in the drill file is not understood and was ignored"));
        }
    }

    fn scale(&self) -> f64 {
        match self.units {
            Units::Mm => 1.0,
            Units::Inch => MM_PER_INCH,
        }
    }

    fn line(&mut self, raw: &str) {
        let (code, comment) = match raw.split_once(';') {
            Some((c, m)) => (c, Some(m)),
            None => (raw, None),
        };
        if let Some(m) = comment {
            self.comment(m);
        }
        let code: String = code.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
        if code.is_empty() {
            return;
        }
        if code == "%" {
            self.in_header = false;
            return;
        }
        let mut chars = code.chars();
        let named = matches!((chars.next(), chars.next()), (Some(a), Some(b)) if a.is_ascii_alphabetic() && b.is_ascii_alphabetic());
        if named {
            self.named(&code);
        } else {
            self.words(&code);
        }
    }

    /// Comments: plating attributes and the format notes writers leave.
    fn comment(&mut self, comment: &str) {
        let lower = comment.trim().to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("#@!") {
            let rest = rest.trim();
            let first_field = |s: &str| s.split(',').next().map(str::trim).unwrap_or("").to_string();
            if let Some(f) = rest.strip_prefix("tf.filefunction,") {
                match first_field(f).as_str() {
                    "plated" => self.file_plated = Some(true),
                    "nonplated" => self.file_plated = Some(false),
                    _ => {}
                }
            } else if let Some(f) = rest.strip_prefix("ta.aperfunction,") {
                match first_field(f).as_str() {
                    "plated" => self.pending_plated = Some(true),
                    "nonplated" => self.pending_plated = Some(false),
                    _ => {}
                }
                let fields: Vec<&str> = f.split(',').map(str::trim).collect();
                if fields.contains(&"viadrill") {
                    self.pending_via = Some(true);
                } else if fields.iter().any(|f| matches!(*f, "componentdrill" | "mechanicaldrill" | "backdrill" | "otherdrill")) {
                    self.pending_via = Some(false);
                }
            } else if rest.starts_with("td") {
                self.pending_plated = None;
                self.pending_via = None;
            }
        } else if let Some(rest) = lower.strip_prefix("format") {
            let rest = rest.trim_start().trim_start_matches('=').trim_start();
            let inner = rest.trim_start_matches('{').split('}').next().unwrap_or("");
            let mut parts = inner.split('/').map(str::trim);
            if let Some(d) = parts.next() {
                self.comment_digits(d);
            }
            for p in parts {
                if p == "decimal" {
                    self.decimal_points = true;
                }
                if !self.units_from_command {
                    match p {
                        "metric" => self.units = Units::Mm,
                        "inch" => self.units = Units::Inch,
                        _ => {}
                    }
                }
                if !self.zeros_from_command {
                    if p.contains("suppress trailing") || p.contains("keep zeros") {
                        self.zeros = Zeros::KeepLeading;
                    } else if p.contains("suppress leading") {
                        self.zeros = Zeros::KeepTrailing;
                    }
                }
            }
        } else if let Some(rest) = lower.strip_prefix("file_format") {
            self.comment_digits(rest.trim_start().trim_start_matches('=').trim());
        } else if let Some(rest) = lower.strip_prefix("type") {
            // A header can list its plated tools under `;TYPE=PLATED` and the rest under
            // `;TYPE=NON_PLATED`: each applies to the tools after it, and the first to the file.
            let plated = match rest.trim_start().trim_start_matches('=').trim() {
                "plated" => Some(true),
                "non_plated" | "nonplated" | "non-plated" => Some(false),
                _ => None,
            };
            if plated.is_some() {
                self.section_plated = plated;
                if self.file_plated.is_none() {
                    self.file_plated = plated;
                }
            }
        }
    }

    /// `a:b` digits from a comment; a header command wins.
    fn comment_digits(&mut self, d: &str) {
        if self.digits_from_command {
            return;
        }
        if let Some((i, f)) = d.split_once(':')
            && let (Ok(i), Ok(f)) = (i.trim().parse::<usize>(), f.trim().parse::<usize>())
            && i <= 9
            && f <= 9
            && i + f > 0
        {
            self.digits = Some((i, f));
        }
    }

    /// Commands that are words: `METRIC,TZ`, `INCH`, `FMAT,2`, `ICI,ON`, `VER,1`, ...
    fn named(&mut self, code: &str) {
        let mut parts = code.split(',');
        let head = parts.next().unwrap_or("");
        match head {
            "METRIC" | "INCH" => {
                self.units = if head == "METRIC" { Units::Mm } else { Units::Inch };
                self.units_from_command = true;
                self.looks_like_drill = true;
                for p in parts {
                    match p {
                        "LZ" => {
                            self.zeros = Zeros::KeepLeading;
                            self.zeros_from_command = true;
                        }
                        "TZ" => {
                            self.zeros = Zeros::KeepTrailing;
                            self.zeros_from_command = true;
                        }
                        _ if !p.is_empty() && p.bytes().all(|b| b == b'0' || b == b'.') => match digit_pattern(p) {
                            Some(d) => {
                                self.digits = Some(d);
                                self.digits_from_command = true;
                            }
                            None => self.warn(format!("the number format `{}` is not usable; the usual digits were assumed", clip(p))),
                        },
                        _ => self.unknown(&format!("{head},{p}")),
                    }
                }
            }
            "FMAT" => {
                if parts.next() != Some("2") {
                    self.warn("the drill file asks for an old command format (FMAT,1); it was read as format 2".into());
                }
            }
            "ICI" => match parts.next() {
                Some("ON") => self.incremental = true,
                Some("OFF") => self.incremental = false,
                _ => self.unknown(code),
            },
            // Machine settings that do not change where the holes are.
            "VER" | "DETECT" | "ATC" | "AFS" | "BLKD" | "TCST" | "CP" | "RSB" | "OSTOP" => {}
            _ => self.unknown(head),
        }
    }

    /// Commands made of letter-number words: `T1C0.8`, `G90`, `X1.5Y2`, `X1Y2G85X3Y2`, ...
    fn words(&mut self, code: &str) {
        let Some(words) = tokenize(code) else {
            self.unknown(code);
            return;
        };
        if words.first().is_some_and(|(l, _)| *l == 'R') {
            self.repeat(code, &words);
            return;
        }
        if let Some(i) = words.iter().position(|(l, v)| *l == 'G' && code_number(v) == Some(85)) {
            let (before, after) = words.split_at(i);
            self.g85(code, before, after.get(1..).unwrap_or(&[]));
            return;
        }
        let (mut x, mut y) = (None, None);
        let (mut tool, mut diameter) = (None, None);
        let mut has_coord = false;
        // Arc centre offsets (`I`, `J`) and radius (`A`), mm.
        let (mut i, mut j, mut radius) = (None, None, None);
        // An unknown G or M code may change what the coordinates mean: they are not used.
        let mut unknown_code = false;
        for &(letter, v) in &words {
            match letter {
                'G' => match code_number(v) {
                    Some(90) => self.incremental = false,
                    Some(91) => self.incremental = true,
                    Some(5) => {
                        self.rout = false;
                        self.tool_down = false;
                    }
                    Some(0) => {
                        self.rout = true;
                        self.motion = Motion::Move;
                    }
                    Some(1) => {
                        self.rout = true;
                        self.motion = Motion::Line;
                    }
                    Some(2) => {
                        self.rout = true;
                        self.motion = Motion::Clockwise;
                    }
                    Some(3) => {
                        self.rout = true;
                        self.motion = Motion::CounterClockwise;
                    }
                    // Canned drill cycle: holes as in drill mode.
                    Some(81) => self.rout = false,
                    _ => {
                        unknown_code = true;
                        self.unknown(&format!("G{v}"));
                    }
                },
                'M' => match code_number(v) {
                    Some(48) => {
                        self.in_header = true;
                        self.looks_like_drill = true;
                    }
                    Some(95) => self.in_header = false,
                    Some(30) => {
                        self.stop = true;
                        return;
                    }
                    Some(15) => self.tool_down = true,
                    Some(16 | 17) => self.tool_down = false,
                    Some(71) => self.units = Units::Mm,
                    Some(72) => self.units = Units::Inch,
                    _ => {
                        unknown_code = true;
                        self.unknown(&format!("M{v}"));
                    }
                },
                'T' => match code_number(v) {
                    Some(n) => tool = Some(n),
                    None => {
                        self.unknown(&format!("T{v}"));
                        return;
                    }
                },
                'C' => match plain_number(v) {
                    Some(d) => diameter = Some((d, v)),
                    None => {
                        self.warn(format!("the tool size `C{}` could not be read", clip(v)));
                        diameter = Some((f64::NAN, v));
                    }
                },
                'X' | 'Y' => {
                    has_coord = true;
                    match self.coord(v) {
                        Some(c) if letter == 'X' => x = Some(c),
                        Some(c) => y = Some(c),
                        None => {
                            self.warn(format!("the coordinate `{letter}{}` could not be read; the line was skipped", clip(v)));
                            return;
                        }
                    }
                }
                'I' | 'J' | 'A' => match self.coord(v) {
                    Some(c) if letter == 'I' => i = Some(c),
                    Some(c) if letter == 'J' => j = Some(c),
                    Some(c) => radius = Some(c),
                    None => {
                        self.warn(format!("the arc value `{letter}{}` could not be read; the line was skipped", clip(v)));
                        return;
                    }
                },
                // Feeds and speeds: they do not move a hole.
                'F' | 'S' => {}
                'B' | 'H' | 'Z' | 'D' if tool.is_some() => {}
                _ => self.unknown(&format!("{letter}{v}")),
            }
        }
        if let Some(n) = tool {
            match diameter {
                Some((d, text)) => {
                    self.define_tool(n, d, text);
                    if !self.in_header {
                        self.tool = Some(n);
                    }
                }
                None => self.tool = (n != 0).then_some(n),
            }
        }
        if has_coord && !unknown_code {
            let Some(target) = self.resolve(self.pos, x, y) else {
                return;
            };
            if !self.rout {
                self.hit(target);
            } else if self.tool_down {
                match self.motion {
                    Motion::Move => {}
                    Motion::Line => self.slot(self.pos, target),
                    Motion::Clockwise => self.arc(self.pos, target, false, (i, j), radius),
                    Motion::CounterClockwise => self.arc(self.pos, target, true, (i, j), radius),
                }
            }
            self.pos = target;
        }
    }

    /// `R<n>X..Y..`: `n` more holes, each stepped by X and Y from the last one.
    fn repeat(&mut self, code: &str, words: &[(char, &str)]) {
        // Any count of plain digits: a huge one saturates and is cut at the entity limit below.
        let count_digits =
            words.first().map(|(_, v)| *v).filter(|v| !v.is_empty() && v.len() <= MAX_NUMBER_CHARS && v.bytes().all(|b| b.is_ascii_digit()));
        let Some(n) = count_digits.map(|v| v.parse::<u64>().unwrap_or(u64::MAX)) else {
            self.warn(format!("the repeat `{}` could not be read; it was skipped", clip(code)));
            return;
        };
        let Some((dx, dy)) = self.xy(code, words.get(1..).unwrap_or(&[])) else {
            return;
        };
        let (dx, dy) = (dx.unwrap_or(0.0), dy.unwrap_or(0.0));
        if self.rout {
            self.warn(format!("the repeat `{}` in rout mode was skipped", clip(code)));
            return;
        }
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        if n == 0 {
            return;
        }
        // Checked once for the whole run, so a huge count costs nothing when nothing is made.
        let Some((diameter, plated, via)) = self.current_tool(n) else {
            return;
        };
        // One more than there is room for, so the limit is reported.
        let count = n.min(MAX_ENTITIES.saturating_sub(self.entities()).saturating_add(1));
        for done in 0..count {
            if !self.room() {
                return;
            }
            let next = (self.pos.0 + dx, self.pos.1 + dy);
            if !in_range(next) {
                self.far = self.far.saturating_add(n.saturating_sub(done));
                return;
            }
            self.out.hits.push(DrillHit { at: Vec2::new(next.0, next.1), diameter, plated, via });
            self.pos = next;
        }
    }

    /// A cut along an arc from `from` to `to`, its centre given by offsets from `from` or by
    /// a radius (positive: the shorter way round, negative: the longer).
    fn arc(&mut self, from: (f64, f64), to: (f64, f64), ccw: bool, ij: (Option<f64>, Option<f64>), radius: Option<f64>) {
        let centre = if ij.0.is_some() || ij.1.is_some() {
            Some((from.0 + ij.0.unwrap_or(0.0), from.1 + ij.1.unwrap_or(0.0)))
        } else {
            radius.and_then(|r| centre_from_radius(from, to, r, ccw))
        };
        let Some(c) = centre.filter(|&c| in_range(c)) else {
            self.bad_arcs = self.bad_arcs.saturating_add(1);
            return;
        };
        let path_radius = (from.0 - c.0).hypot(from.1 - c.1);
        if !(path_radius > 1e-9) || !((to.0 - c.0).hypot(to.1 - c.1) > 1e-9) {
            self.bad_arcs = self.bad_arcs.saturating_add(1);
            return;
        }
        if !self.room() {
            return;
        }
        let Some((diameter, plated, _)) = self.current_tool(1) else {
            return;
        };
        if !(path_radius - diameter / 2.0 > 1e-9) {
            self.bad_arcs = self.bad_arcs.saturating_add(1);
            return;
        }
        let (a, b) = if ccw { (from, to) } else { (to, from) };
        self.out.arcs.push(DrillArc { c: Vec2::new(c.0, c.1), a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1), diameter, plated });
    }

    /// `X..Y..G85X..Y..`: a slot from the first position to the second.
    fn g85(&mut self, code: &str, before: &[(char, &str)], after: &[(char, &str)]) {
        let Some((x0, y0)) = self.xy(code, before) else {
            return;
        };
        let Some((x1, y1)) = self.xy(code, after) else {
            return;
        };
        let Some(start) = self.resolve(self.pos, x0, y0) else {
            return;
        };
        let Some(end) = self.resolve(start, x1, y1) else {
            return;
        };
        self.slot(start, end);
        self.pos = end;
    }

    /// The X and Y of a run of words that should hold nothing else.
    fn xy(&mut self, code: &str, words: &[(char, &str)]) -> Option<(Option<f64>, Option<f64>)> {
        let (mut x, mut y) = (None, None);
        for &(letter, v) in words {
            let c = match letter {
                'X' | 'Y' => self.coord(v),
                _ => None,
            };
            match (letter, c) {
                ('X', Some(c)) => x = Some(c),
                ('Y', Some(c)) => y = Some(c),
                _ => {
                    self.warn(format!("the line `{}` could not be read; it was skipped", clip(code)));
                    return None;
                }
            }
        }
        Some((x, y))
    }

    fn define_tool(&mut self, n: u32, d: f64, text: &str) {
        if !self.tools.contains_key(&n) && self.tools.len() >= MAX_TOOLS {
            if !self.too_many_tools_warned {
                self.too_many_tools_warned = true;
                self.warn(format!("the drill file defines more than {MAX_TOOLS} tools; the rest were ignored"));
            }
            return;
        }
        self.looks_like_drill = true;
        let mm = d * self.scale();
        let plated = self.pending_plated.take().or(self.section_plated);
        let via = self.pending_via.take();
        let tool = if mm.is_finite() && mm > 0.0 && mm <= MAX_DIAMETER_MM {
            Tool::Ok { diameter: mm, plated, via }
        } else {
            if d.is_finite() {
                self.warn(format!("tool T{n} has an unusable size `C{}`; its holes are skipped", clip(text)));
            }
            Tool::Refused
        };
        self.tools.insert(n, tool);
    }

    /// The current tool's diameter, plating and via flag, or `None` when holes cannot be made
    /// (the `count` holes that wanted it are counted for the warnings).
    fn current_tool(&mut self, count: usize) -> Option<(f64, Option<bool>, Option<bool>)> {
        let Some(n) = self.tool else {
            self.no_tool = self.no_tool.saturating_add(count);
            return None;
        };
        match self.tools.get(&n).copied() {
            Some(Tool::Ok { diameter, plated, via }) => Some((diameter, plated.or(self.file_plated), via)),
            Some(Tool::Refused) => {
                let c = self.refused.entry(n).or_insert(0);
                *c = c.saturating_add(count);
                None
            }
            None => {
                let c = self.undefined.entry(n).or_insert(0);
                *c = c.saturating_add(count);
                None
            }
        }
    }

    fn entities(&self) -> usize {
        self.out.hits.len().saturating_add(self.out.slots.len()).saturating_add(self.out.arcs.len())
    }

    /// Room for one more hole or slot; stops reading when the file has too many.
    fn room(&mut self) -> bool {
        if self.entities() >= MAX_ENTITIES {
            self.warn(format!("the drill file has more than {MAX_ENTITIES} holes and slots; the rest were skipped"));
            self.stop = true;
            return false;
        }
        true
    }

    fn hit(&mut self, at: (f64, f64)) {
        if !self.room() {
            return;
        }
        self.looks_like_drill = true;
        if let Some((diameter, plated, via)) = self.current_tool(1) {
            self.out.hits.push(DrillHit { at: Vec2::new(at.0, at.1), diameter, plated, via });
        }
    }

    fn slot(&mut self, a: (f64, f64), b: (f64, f64)) {
        if !self.room() {
            return;
        }
        self.looks_like_drill = true;
        if let Some((diameter, plated, _)) = self.current_tool(1) {
            self.out.slots.push(DrillSlot { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1), diameter, plated });
        }
    }

    /// The position the given coordinates (mm) name from `from`: absolute or incremental, a
    /// missing axis keeping its value. `None` (counted) when it is out of range.
    fn resolve(&mut self, from: (f64, f64), x: Option<f64>, y: Option<f64>) -> Option<(f64, f64)> {
        let axis = |old: f64, new: Option<f64>, inc: bool| match new {
            Some(v) if inc => old + v,
            Some(v) => v,
            None => old,
        };
        let p = (axis(from.0, x, self.incremental), axis(from.1, y, self.incremental));
        if in_range(p) {
            Some(p)
        } else {
            self.far = self.far.saturating_add(1);
            None
        }
    }

    /// A coordinate in millimetres.
    fn coord(&self, v: &str) -> Option<f64> {
        let (negative, body) = split_sign(v)?;
        let value = if body.contains('.') || self.decimal_points {
            plain_number(body)?
        } else {
            let (int, dec) = self.digits.unwrap_or_else(|| {
                let usual = match self.units {
                    Units::Mm => (3, 3),
                    Units::Inch => (2, 4),
                };
                self.guessed.set(Some(usual.1));
                usual
            });
            implied(body, int, dec, self.zeros)?
        };
        let mm = if negative { -value } else { value } * self.scale();
        mm.is_finite().then_some(mm)
    }

    fn finish(mut self) -> Result<Drills> {
        self.out.guessed_decimals = self.guessed.get();
        if self.no_tool > 0 {
            let n = self.no_tool;
            self.warn(format!("{n} hole(s) came before any tool was chosen; they were skipped"));
        }
        for (t, n) in std::mem::take(&mut self.undefined) {
            self.warn(format!("{n} hole(s) use tool T{t}, which the file never defines; they were skipped"));
        }
        for (t, n) in std::mem::take(&mut self.refused) {
            self.warn(format!("{n} hole(s) use tool T{t}, whose size was refused; they were skipped"));
        }
        if self.bad_arcs > 0 {
            let n = self.bad_arcs;
            self.warn(format!("{n} routed arc(s) had no usable centre (missing, unreachable, or tighter than the tool); they were skipped"));
        }
        if self.far > 0 {
            let n = self.far;
            self.warn(format!("{n} position(s) lie more than {MAX_COORD_MM} mm from the origin; they were skipped"));
        }
        if self.dropped_warnings > 0 {
            let n = self.dropped_warnings;
            self.out.warnings.push(format!("... and {n} more warning(s)"));
        }
        if self.out.hits.is_empty() && self.out.slots.is_empty() && self.out.arcs.is_empty() {
            if !self.looks_like_drill {
                return Err(IoError::Invalid("this is not an Excellon drill file: it has no drill header, tools or holes".into()));
            }
            // An empty drill file is normal (a board without unplated holes, say).
            self.out.warnings.insert(0, "the drill file has no holes or slots".into());
        }
        Ok(self.out)
    }
}

fn in_range(p: (f64, f64)) -> bool {
    p.0.is_finite() && p.1.is_finite() && p.0.abs() <= MAX_COORD_MM && p.1.abs() <= MAX_COORD_MM
}

/// The centre of an arc from `a` to `b` with radius `r` (positive: the shorter way round,
/// negative: the longer), turning counter-clockwise when `ccw`. `None` when no circle of that
/// radius reaches both ends.
fn centre_from_radius(a: (f64, f64), b: (f64, f64), r: f64, ccw: bool) -> Option<(f64, f64)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let d = dx.hypot(dy);
    let half = d / 2.0;
    if !(d > 1e-9) || !r.is_finite() || r.abs() < half - 1e-9 {
        return None;
    }
    let h = (r * r - half * half).max(0.0).sqrt();
    // The shorter arc turning left has its centre on the left of the chord.
    let side = if ccw == (r > 0.0) { h } else { -h };
    let (lx, ly) = (-dy / d, dx / d);
    Some(((a.0 + b.0) / 2.0 + lx * side, (a.1 + b.1) / 2.0 + ly * side))
}

/// Whether any X or Y coordinate in the file is written with a decimal point.
fn writes_decimal_points(text: &str) -> bool {
    text.split(['\n', '\r']).any(|line| {
        let b = line.split(';').next().unwrap_or("").as_bytes();
        let mut i = 0;
        while let Some(&c) = b.get(i) {
            i += 1;
            if matches!(c, b'X' | b'Y' | b'x' | b'y') {
                while b.get(i).is_some_and(|d| d.is_ascii_digit() || matches!(d, b'+' | b'-')) {
                    i += 1;
                }
                if b.get(i) == Some(&b'.') {
                    return true;
                }
            }
        }
        false
    })
}

/// Split a code into letter-number words; `None` when it holds anything else.
fn tokenize(code: &str) -> Option<Vec<(char, &str)>> {
    if !code.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    let mut out = Vec::new();
    let mut rest = code;
    while let Some(letter) = rest.chars().next() {
        if !letter.is_ascii_alphabetic() {
            return None;
        }
        let tail = rest.get(1..)?;
        let end = tail.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(tail.len());
        out.push((letter, tail.get(..end)?));
        rest = tail.get(end..)?;
    }
    Some(out)
}

/// The number of a G, M or T code: plain digits only.
fn code_number(v: &str) -> Option<u32> {
    if v.is_empty() || v.len() > 9 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    v.parse().ok()
}

/// `-` or `+` then the rest.
fn split_sign(v: &str) -> Option<(bool, &str)> {
    if let Some(rest) = v.strip_prefix('-') { Some((true, rest)) } else { Some((false, v.strip_prefix('+').unwrap_or(v))) }
}

/// A number written with an optional sign, digits and at most one decimal point.
fn plain_number(v: &str) -> Option<f64> {
    let (negative, body) = split_sign(v)?;
    if body.is_empty() || body.len() > MAX_NUMBER_CHARS {
        return None;
    }
    if !body.bytes().all(|b| b.is_ascii_digit() || b == b'.') || body.bytes().filter(|&b| b == b'.').count() > 1 {
        return None;
    }
    if !body.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    // A leading and a trailing zero make ".5" and "5." readable without changing the value.
    let text = if body.contains('.') { format!("0{body}0") } else { format!("0{body}.0") };
    let value: f64 = text.parse().ok()?;
    let value = if negative { -value } else { value };
    value.is_finite().then_some(value)
}

/// An implied-decimal integer with `int` integer and `dec` decimal digits.
fn implied(digits: &str, int: usize, dec: usize, zeros: Zeros) -> Option<f64> {
    if digits.is_empty() || digits.len() > MAX_NUMBER_CHARS || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut d = digits.to_string();
    let split = match zeros {
        Zeros::KeepLeading => {
            while d.len() < int {
                d.push('0');
            }
            int
        }
        Zeros::KeepTrailing => {
            while d.len() < dec {
                d.insert(0, '0');
            }
            d.len().checked_sub(dec)?
        }
    };
    let whole = d.get(..split)?;
    let fraction = d.get(split..)?;
    let value: f64 = format!("0{whole}.{fraction}0").parse().ok()?;
    value.is_finite().then_some(value)
}

/// `000.000`-style digit pattern: integer and decimal digit counts.
fn digit_pattern(p: &str) -> Option<(usize, usize)> {
    if p.is_empty() || !p.bytes().all(|b| b == b'0' || b == b'.') {
        return None;
    }
    let (i, f) = p.split_once('.').unwrap_or((p, ""));
    if f.contains('.') || i.len() > 9 || f.len() > 9 || i.len() + f.len() == 0 {
        return None;
    }
    Some((i.len(), f.len()))
}

/// Shorten file text quoted in a warning.
fn clip(s: &str) -> String {
    let mut out: String = s.chars().take(24).collect();
    if s.chars().nth(24).is_some() {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
#[path = "excellon_tests.rs"]
mod tests;
