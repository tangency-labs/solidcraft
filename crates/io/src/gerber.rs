//! Gerber (RS-274X) fabrication layers of a printed circuit board.
//!
//! The text is parsed by the `gerber_parser` crate (into `gerber-types` commands). Before that,
//! a pre-pass puts every command on its own line (the crate reads one command per line) and
//! keeps away input the crate cannot take without panicking: non-ASCII commands, coordinates
//! too long for its 64-bit arithmetic, formats with more than 6 decimals, and attributes
//! (whose parser indexes missing arguments). The pre-pass reads the file attributes itself.
//!
//! This slice reads a board outline layer (the profile the board is cut along): the centre lines
//! of its draws and arcs, or the boundary of a region, in millimetres with y up. Aperture shapes
//! do not matter for a centre line, so apertures are selected but never read, and aperture
//! macros are left out.

use std::io::BufReader;

use gerber_parser::gerber_types::{
    ApertureBlock, AxisSelect, Command, CoordinateMode, CoordinateNumber, DCode, ExtendedCode, FunctionCode, GCode, ImageMirroring, ImageRotation,
    InterpolationMode, MCode, Operation, QuadrantMode, StepAndRepeat, Unit,
};
use gerber_parser::{ContentError, GerberParserErrorWithContext};
use solidcore_record::Vec2;

use crate::sketch2d::{MAX_DRAWING_BYTES, MAX_ENTITIES};
use crate::{Geom2, IoError, Result};

/// A board outline layer (the profile the board is cut along), in millimetres, y up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GerberOutline {
    /// The drawn path's centre lines: lines and arcs, end points exactly as the file gives them.
    pub geom: Vec<Geom2>,
    /// Things read but not understood, or guessed, worded for a person.
    pub warnings: Vec<String>,
}

/// Farthest a coordinate may be from the origin, in millimetres (a kilometre).
const MAX_COORD_MM: f64 = 1e6;
/// Most digits one coordinate may have: the specification allows 6 + 6, and the parser scales a
/// coordinate to millionths in an `i64`, which 12 digits always fit.
const MAX_DIGITS: usize = 12;
/// Most decimals a format may have: the parser cannot scale more than 6.
const MAX_DECIMALS: u32 = 6;
/// Most distinct warnings kept; the rest are counted.
const MAX_WARNINGS: usize = 50;
/// Most characters of a command quoted in a warning.
const MAX_QUOTE: usize = 40;
const MM_PER_INCH: f64 = 25.4;
/// An arc whose sweep is at most this is within one quadrant (single-quadrant mode).
const QUADRANT: f64 = std::f64::consts::FRAC_PI_2 + 1e-6;

/// Read a Gerber board outline layer.
pub fn read_gerber_outline(text: &str) -> Result<GerberOutline> {
    if text.len() > MAX_DRAWING_BYTES {
        return Err(IoError::Invalid(format!("the Gerber file is too large (over {} MiB)", MAX_DRAWING_BYTES >> 20)));
    }
    let mut r = Reader::default();
    let lines = r.prepare(text)?;
    let doc = match gerber_parser::parse(BufReader::new(lines.as_bytes())) {
        Ok(doc) => doc,
        Err((doc, e)) => {
            r.warn(format!("The Gerber file could not be read to the end: {e}"));
            doc
        }
    };
    for command in doc.commands {
        match command {
            Ok(c) => r.command(&c)?,
            Err(e) => r.library_error(&e),
        }
        if r.ended {
            break;
        }
    }
    r.finish()
}

// ---- pre-pass: one command per line, nothing the parser cannot take ----

/// One data block: a word command (`X100Y200D01`) or the inside of a `%...%` command.
enum Block<'a> {
    Word(&'a str),
    Extended(&'a str),
}

/// A block the file does not finish.
enum Broken<'a> {
    Extended,
    Word(&'a str),
}

/// Splits the file into blocks. Positions only ever land just past ASCII `*` / `%` or on a
/// whitespace boundary, so every slice is on a character boundary.
struct Blocks<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Iterator for Blocks<'a> {
    type Item = std::result::Result<Block<'a>, Broken<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.text.get(self.pos..)?;
        let trimmed = rest.trim_start();
        if trimmed.is_empty() {
            self.pos = self.text.len();
            return None;
        }
        let start = self.pos + (rest.len() - trimmed.len());
        if let Some(body) = trimmed.strip_prefix('%') {
            return Some(match body.find('%') {
                Some(end) => {
                    self.pos = start + end + 2;
                    Ok(Block::Extended(body.get(..end)?))
                }
                None => {
                    self.pos = self.text.len();
                    Err(Broken::Extended)
                }
            });
        }
        Some(match trimmed.find(['*', '%']) {
            Some(end) if trimmed.as_bytes().get(end) == Some(&b'*') => {
                self.pos = start + end + 1;
                Ok(Block::Word(trimmed.get(..end)?))
            }
            Some(end) => {
                self.pos = start + end;
                Err(Broken::Word(trimmed.get(..end)?))
            }
            None => {
                self.pos = self.text.len();
                Err(Broken::Word(trimmed))
            }
        })
    }
}

/// A signed coordinate as written: negative, digits.
type Number<'a> = (bool, &'a str);

/// The words of one block.
#[derive(Default)]
struct Words<'a> {
    g: Vec<u32>,
    d: Option<u32>,
    m: Option<u32>,
    x: Option<Number<'a>>,
    y: Option<Number<'a>>,
    i: Option<Number<'a>>,
    j: Option<Number<'a>>,
}

impl Words<'_> {
    fn coordinates(&self) -> impl Iterator<Item = (char, Number<'_>)> {
        [('X', self.x), ('Y', self.y), ('I', self.i), ('J', self.j)].into_iter().filter_map(|(letter, n)| n.map(|n| (letter, n)))
    }
}

/// Is this a line `gerber_parser` 0.5 reads without slicing out of bounds? It slices `G0...`
/// lines at byte 3.
fn safe_for_parser(line: &str) -> bool {
    line.is_ascii() && !(line.starts_with("G0") && line.len() < 4)
}

// ---- reading the parsed commands ----

#[derive(Clone, Copy, PartialEq, Default)]
enum Quadrant {
    #[default]
    Unset,
    Single,
    Multi,
}

#[derive(Default)]
struct Warnings {
    /// Distinct messages and how often each came up, in order of first appearance.
    seen: Vec<(String, usize)>,
    dropped: usize,
}

impl Warnings {
    fn add(&mut self, msg: String) {
        if let Some(entry) = self.seen.iter_mut().find(|(m, _)| *m == msg) {
            entry.1 = entry.1.saturating_add(1);
        } else if self.seen.len() < MAX_WARNINGS {
            self.seen.push((msg, 1));
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    fn into_vec(self) -> Vec<String> {
        let mut out: Vec<String> = self.seen.into_iter().map(|(m, n)| if n > 1 { format!("{m} ({n} times)") } else { m }).collect();
        if self.dropped > 0 {
            out.push(format!("{} more warnings were not listed.", self.dropped));
        }
        out
    }
}

#[derive(Default)]
struct Reader {
    // Pre-pass.
    /// A format statement came before (coordinates need it).
    seen_format: bool,
    /// The format statement asks for incremental coordinates.
    prep_incremental: bool,
    /// The last X written (sign and digits), for arcs that give only their offsets.
    last_x: String,
    file_function: Option<String>,
    /// The file has an end command (M00/M01/M02).
    has_end: bool,
    // Commands.
    /// Millimetres per file unit, once `%MO..%` (or G70/G71) says.
    unit: Option<f64>,
    incremental: bool,
    interpolation: Option<InterpolationMode>,
    quadrant: Quadrant,
    current: Vec2,
    in_region: bool,
    /// Where the region's current contour started, once it draws.
    contour_start: Option<Vec2>,
    /// Nesting depth of block apertures (`%AB%`): their draws define an aperture, not the image.
    block_depth: usize,
    flashes: usize,
    ended: bool,
    geom: Vec<Geom2>,
    warnings: Warnings,
}

impl Reader {
    fn warn(&mut self, msg: String) {
        self.warnings.add(msg);
    }

    fn unknown(&mut self, what: &str) {
        self.warn(format!("Skipped a command this reader does not know: `{}`", quote(what)));
    }

    /// The file rewritten for the parser: one command per line, each one it can take.
    fn prepare(&mut self, text: &str) -> Result<String> {
        let mut out = String::with_capacity(text.len().saturating_add(text.len() / 4));
        for item in (Blocks { text, pos: 0 }) {
            let lines = match item {
                Ok(Block::Word(w)) => self.prepare_word(w)?,
                Ok(Block::Extended(e)) => self.prepare_extended(e)?,
                Err(Broken::Extended) => {
                    self.warn("The file ends inside a `%` command; the rest was skipped.".into());
                    Vec::new()
                }
                Err(Broken::Word(w)) => {
                    self.warn(format!("A command is missing its closing `*` and was skipped: `{}`", quote(w)));
                    Vec::new()
                }
            };
            for line in lines {
                if safe_for_parser(&line) {
                    out.push_str(&line);
                    out.push('\n');
                } else {
                    self.unknown(&line);
                }
            }
            if self.has_end {
                break;
            }
        }
        Ok(out)
    }

    fn prepare_word(&mut self, w: &str) -> Result<Vec<String>> {
        let w = w.trim();
        if w.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(comment) = comment_text(w) {
            // Attributes written as comments (`G04 #@! TF.FileFunction,Profile,NP*`); nothing
            // else in a comment matters.
            if let Some(attr) = comment.trim_start().strip_prefix("#@!") {
                self.attribute(attr.trim());
            }
            return Ok(Vec::new());
        }
        if !w.is_ascii() {
            self.warn(format!("Skipped a command with characters that are not plain ASCII: `{}`", quote(w)));
            return Ok(Vec::new());
        }
        let compact: String = w.chars().filter(|c| !c.is_whitespace()).collect();
        let Some(words) = split_words(&compact) else {
            self.unknown(w);
            return Ok(Vec::new());
        };
        let mut lines = Vec::new();
        for &g in &words.g {
            match g {
                // A comment after another G code: the rest of the block is text.
                4 => return Ok(lines),
                // Deprecated "prepare flash" before a D03 (the D03 says it already), and
                // absolute coordinates, the parser's default.
                55 | 90 => {}
                1 | 2 | 3 | 36 | 37 | 54 | 74 | 75 => lines.push(format!("G{g:02}*")),
                // Deprecated units; the parser reads only the extended form.
                70 => lines.push("%MOIN*%".into()),
                71 => lines.push("%MOMM*%".into()),
                91 => {
                    return Err(IoError::Invalid("the Gerber file switches to incremental coordinates with G91, which is not supported".into()));
                }
                _ => self.warn(format!("Skipped an unknown G code G{g:02}.")),
            }
        }
        for (_, (_, digits)) in words.coordinates() {
            if digits.len() > MAX_DIGITS {
                return Err(IoError::Invalid(format!("a Gerber coordinate has {} digits; at most {MAX_DIGITS} make sense", digits.len())));
            }
        }
        let has_coords = words.coordinates().next().is_some();
        if has_coords && !self.seen_format {
            return Err(IoError::Invalid("a Gerber coordinate comes before the format statement (%FS...%), so its scale is unknown".into()));
        }
        let mut op = String::new();
        // The parser reads an operation only when it starts with X or Y: an arc given by its
        // offsets alone gets the current X back.
        if words.x.is_none() && words.y.is_none() && (words.i.is_some() || words.j.is_some()) {
            op.push('X');
            op.push_str(if self.prep_incremental || self.last_x.is_empty() { "0" } else { &self.last_x });
        }
        for (letter, (negative, digits)) in words.coordinates() {
            op.push(letter);
            if negative {
                op.push('-');
            }
            op.push_str(digits);
        }
        if let Some((negative, digits)) = words.x {
            self.last_x = format!("{}{digits}", if negative { "-" } else { "" });
        }
        match words.d {
            Some(d @ 1..=3) => op.push_str(&format!("D0{d}")),
            Some(d) if (10..=i32::MAX.unsigned_abs()).contains(&d) => {
                if has_coords {
                    self.warn("Coordinates together with an aperture selection were skipped.".into());
                    op.clear();
                }
                lines.push(format!("D{d}*"));
            }
            Some(d) => {
                self.warn(format!("Skipped a block with the D code D{d:02}, which selects nothing."));
                return Ok(lines);
            }
            None if has_coords => {
                self.warn("Coordinates without an operation code reuse the previous one (deprecated); read anyway.".into());
            }
            None => {}
        }
        if !op.is_empty() {
            op.push('*');
            lines.push(op);
        }
        if let Some(m) = words.m {
            match m {
                0..=2 => {
                    self.has_end = true;
                    lines.push("M02*".into());
                }
                _ => self.warn(format!("Skipped an unknown M code M{m:02}.")),
            }
        }
        Ok(lines)
    }

    fn prepare_extended(&mut self, body: &str) -> Result<Vec<String>> {
        let body: String = body.chars().filter(|c| *c != '\r' && *c != '\n').collect();
        let body = body.trim();
        if !body.is_ascii() {
            self.warn(format!("Skipped a command with characters that are not plain ASCII: `%{}%`", quote(body)));
            return Ok(Vec::new());
        }
        // An aperture macro's body has `*` inside; aperture shapes do not change a centre line.
        if body.starts_with("AM") {
            return Ok(Vec::new());
        }
        let mut lines = Vec::new();
        for cmd in body.split('*') {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                continue;
            }
            match cmd.get(..2).unwrap_or(cmd) {
                "FS" => lines.push(self.format_statement(cmd)?),
                "MO" => match cmd {
                    "MOMM" | "MOIN" => lines.push(format!("%{cmd}*%")),
                    _ => {
                        return Err(IoError::Invalid(format!("the Gerber file uses an unknown unit `{}` (expected %MOMM*% or %MOIN*%)", quote(cmd))));
                    }
                },
                "TF" => self.attribute(cmd),
                // Other attributes, and the deprecated layer name, say nothing about geometry.
                "TA" | "TO" | "TD" | "LN" => {}
                "AD" | "AB" | "LP" | "LM" | "LR" | "LS" | "SR" | "IP" | "IN" | "OF" | "SF" | "MI" | "IR" | "AS" => {
                    lines.push(format!("%{cmd}*%"));
                }
                _ => self.warn(format!("Skipped a command this reader does not know: `%{}%`", quote(cmd))),
            }
        }
        Ok(lines)
    }

    /// Checks `%FS<L|T|D><A|I>[N.][G.][D.][M.]X<i><d>Y<i><d>*%` and writes it in the one form the
    /// parser reads.
    fn format_statement(&mut self, cmd: &str) -> Result<String> {
        let bad = |why: &str| IoError::Invalid(format!("the Gerber format statement `%{}%` cannot be read{why}", quote(cmd)));
        let b = cmd.as_bytes();
        let trailing = match b.get(2) {
            Some(b'L' | b'D') => false,
            Some(b'T') => true,
            _ => return Err(bad("")),
        };
        let incremental = match b.get(3) {
            Some(b'A') => false,
            Some(b'I') => true,
            _ => return Err(bad("")),
        };
        let digit = |k: usize| b.get(k).filter(|c| c.is_ascii_digit()).map(|c| u32::from(c - b'0'));
        let (mut x, mut y) = (None, None);
        let mut k = 4;
        while let Some(&letter) = b.get(k) {
            match letter {
                b'X' | b'Y' => {
                    let pair = digit(k + 1).zip(digit(k + 2)).ok_or_else(|| bad(""))?;
                    if letter == b'X' {
                        x = Some(pair);
                    } else {
                        y = Some(pair);
                    }
                    k += 3;
                }
                // Deprecated sequence-number, G-code, D-code and M-code lengths.
                b'N' | b'G' | b'D' | b'M' => {
                    digit(k + 1).ok_or_else(|| bad(""))?;
                    k += 2;
                }
                _ => return Err(bad("")),
            }
        }
        let (x, y) = x.zip(y).ok_or_else(|| bad(""))?;
        if x != y {
            return Err(bad(": X and Y have different numbers of digits"));
        }
        let (int, dec) = x;
        if !(1..=6).contains(&int) || dec > MAX_DECIMALS {
            return Err(bad(": it needs 1 to 6 integer digits and at most 6 decimals"));
        }
        if trailing {
            self.warn("The file omits trailing zeros, a deprecated coordinate format; read anyway.".into());
        }
        if incremental {
            self.warn("The file uses incremental coordinates, a deprecated coordinate format; read anyway.".into());
        }
        self.seen_format = true;
        self.prep_incremental = incremental;
        let (zeros, mode) = (if trailing { 'T' } else { 'L' }, if incremental { 'I' } else { 'A' });
        Ok(format!("%FS{zeros}{mode}X{int}{dec}Y{int}{dec}*%"))
    }

    /// A file attribute, `TF.<name>,<value>...` (from `%TF...%` or a `G04 #@! TF...` comment).
    fn attribute(&mut self, cmd: &str) {
        let Some(rest) = cmd.strip_prefix("TF") else { return };
        let mut parts = rest.splitn(2, ',');
        if parts.next().map(str::trim) == Some(".FileFunction") {
            self.file_function = Some(parts.next().unwrap_or("").trim().to_string());
        }
    }

    fn library_error(&mut self, e: &GerberParserErrorWithContext) {
        // The end is checked by the pre-pass, which words it for a person.
        if matches!(e.error, ContentError::NoEndOfFile) {
            return;
        }
        match &e.line {
            Some((_, line)) => self.warn(format!("Skipped a command that could not be read: `{}`", quote(line))),
            None => self.warn(format!("Skipped a command that could not be read ({})", quote(&e.error.to_string()))),
        }
    }

    fn finish(mut self) -> Result<GerberOutline> {
        if self.in_region {
            self.close_contour();
            self.warn("The file ends inside a region (G36 without G37).".into());
        }
        if !self.has_end {
            self.warn("The file has no end command (M02); it may be cut short.".into());
        }
        if let Some(function) = self.file_function.take() {
            let kind = function.split(',').next().unwrap_or("").trim();
            if kind != "Profile" {
                self.warn(format!(
                    "The file says it is a `{}` layer, not a board outline (Profile); its drawing was read as an outline anyway.",
                    quote(&function)
                ));
            }
        }
        if self.geom.is_empty() {
            let hint = if self.flashes > 0 { " It only flashes apertures (pads), so it is probably not the board outline layer." } else { "" };
            return Err(IoError::Invalid(format!("the Gerber file draws no lines or arcs, so there is no board outline in it.{hint}")));
        }
        Ok(GerberOutline { geom: self.geom, warnings: self.warnings.into_vec() })
    }

    fn push(&mut self, g: Geom2) -> Result<()> {
        if self.geom.len() >= MAX_ENTITIES {
            return Err(IoError::Invalid(format!("the Gerber outline has more than {MAX_ENTITIES} lines and arcs")));
        }
        self.geom.push(g);
        Ok(())
    }

    fn command(&mut self, c: &Command) -> Result<()> {
        match c {
            Command::FunctionCode(FunctionCode::GCode(g)) => self.g_code(g),
            Command::FunctionCode(FunctionCode::DCode(DCode::Operation(op))) => return self.operate(op),
            // Selects an aperture; its shape does not change a centre line.
            Command::FunctionCode(FunctionCode::DCode(DCode::SelectAperture(_))) => {}
            Command::FunctionCode(FunctionCode::MCode(MCode::EndOfFile)) => self.ended = true,
            Command::ExtendedCode(e) => self.extended(e),
        }
        Ok(())
    }

    fn g_code(&mut self, g: &GCode) {
        match g {
            GCode::InterpolationMode(m) => self.interpolation = Some(*m),
            GCode::RegionMode(true) => {
                if self.in_region {
                    self.close_contour();
                    self.warn("A region starts inside another region (G36 twice).".into());
                }
                self.in_region = true;
                self.contour_start = None;
            }
            GCode::RegionMode(false) => {
                if self.in_region {
                    self.close_contour();
                }
                self.in_region = false;
            }
            GCode::QuadrantMode(QuadrantMode::Single) => self.quadrant = Quadrant::Single,
            GCode::QuadrantMode(QuadrantMode::Multi) => self.quadrant = Quadrant::Multi,
            GCode::Unit(u) => self.unit = Some(unit_mm(*u)),
            GCode::CoordinateMode(m) => self.incremental = *m == CoordinateMode::Incremental,
            GCode::Comment(_) | GCode::SelectAperture => {}
        }
    }

    fn extended(&mut self, e: &ExtendedCode) {
        match e {
            ExtendedCode::CoordinateFormat(fs) => self.incremental = fs.coordinate_mode == CoordinateMode::Incremental,
            ExtendedCode::Unit(u) => self.unit = Some(unit_mm(*u)),
            ExtendedCode::ApertureBlock(ApertureBlock::Open { .. }) => {
                self.block_depth = self.block_depth.saturating_add(1);
            }
            ExtendedCode::ApertureBlock(ApertureBlock::Close) => self.block_depth = self.block_depth.saturating_sub(1),
            ExtendedCode::StepAndRepeat(StepAndRepeat::Open { repeat_x, repeat_y, .. }) if *repeat_x > 1 || *repeat_y > 1 => {
                self.warn("Step and repeat (%SR%) is not supported; the repeated part was read once.".into());
            }
            ExtendedCode::MirrorImage(m) if *m != ImageMirroring::None => self.image_setting("MI"),
            ExtendedCode::RotateImage(r) if *r != ImageRotation::None => self.image_setting("IR"),
            ExtendedCode::OffsetImage(o) if o.a != 0.0 || o.b != 0.0 => self.image_setting("OF"),
            ExtendedCode::ScaleImage(s) if s.a != 1.0 || s.b != 1.0 => self.image_setting("SF"),
            ExtendedCode::AxisSelect(a) if *a != AxisSelect::AXBY => self.image_setting("AS"),
            // Apertures, polarity, aperture transformations, attributes, image name and polarity:
            // none of them moves a centre line.
            _ => {}
        }
    }

    fn image_setting(&mut self, code: &str) {
        self.warn(format!("The deprecated image setting %{code}% is not supported; it was ignored."));
    }

    /// Millimetres per file unit; assumes millimetres (with a warning) when the file never says.
    fn unit(&mut self) -> f64 {
        match self.unit {
            Some(u) => u,
            None => {
                self.warn("The file does not give its unit (%MO..%); millimetres were assumed.".into());
                self.unit = Some(1.0);
                1.0
            }
        }
    }

    /// A coordinate in millimetres.
    fn mm(&mut self, n: CoordinateNumber) -> Result<f64> {
        let mm = f64::from(n) * self.unit();
        check_range(mm)?;
        Ok(mm)
    }

    /// The point a coordinate pair leads to: modal (a missing axis keeps its value) and, in
    /// incremental mode, relative to the current point.
    fn target(&mut self, x: Option<CoordinateNumber>, y: Option<CoordinateNumber>) -> Result<Vec2> {
        let mut t = self.current;
        if let Some(x) = x {
            let v = self.mm(x)?;
            t.x = if self.incremental { v + self.current.x } else { v };
        }
        if let Some(y) = y {
            let v = self.mm(y)?;
            t.y = if self.incremental { v + self.current.y } else { v };
        }
        check_range(t.x)?;
        check_range(t.y)?;
        Ok(t)
    }

    fn operate(&mut self, op: &Operation) -> Result<()> {
        match op {
            Operation::Interpolate(coords, offset) => {
                let (x, y) = coords.as_ref().map_or((None, None), |c| (c.x, c.y));
                let target = self.target(x, y)?;
                let (mut di, mut dj) = (0.0, 0.0);
                if let Some(o) = offset {
                    if let Some(i) = o.x {
                        di = self.mm(i)?;
                    }
                    if let Some(j) = o.y {
                        dj = self.mm(j)?;
                    }
                }
                let start = self.current;
                self.current = target;
                self.draw(start, target, di, dj)
            }
            Operation::Move(coords) => {
                let (x, y) = coords.as_ref().map_or((None, None), |c| (c.x, c.y));
                let target = self.target(x, y)?;
                if self.in_region {
                    self.close_contour();
                }
                self.current = target;
                Ok(())
            }
            Operation::Flash(coords) => {
                let (x, y) = coords.as_ref().map_or((None, None), |c| (c.x, c.y));
                self.current = self.target(x, y)?;
                if self.block_depth == 0 {
                    self.flashes = self.flashes.saturating_add(1);
                    self.warn("Flashed apertures (D03, pads) are not part of an outline and were skipped.".into());
                }
                Ok(())
            }
        }
    }

    /// Ends the region's current contour, warning when it does not return to its start.
    fn close_contour(&mut self) {
        if let Some(s) = self.contour_start.take()
            && s != self.current
        {
            self.warn("A region's contour does not end where it starts.".into());
        }
    }

    fn draw(&mut self, start: Vec2, end: Vec2, di: f64, dj: f64) -> Result<()> {
        if self.block_depth > 0 {
            return Ok(());
        }
        if self.in_region && self.contour_start.is_none() {
            self.contour_start = Some(start);
        }
        let mode = match self.interpolation {
            Some(m) => m,
            None => {
                self.warn("The file draws before choosing an interpolation mode; straight lines (G01) were assumed.".into());
                self.interpolation = Some(InterpolationMode::Linear);
                InterpolationMode::Linear
            }
        };
        match mode {
            InterpolationMode::Linear => {
                if start != end {
                    self.push(Geom2::Line(start, end))?;
                }
                Ok(())
            }
            InterpolationMode::ClockwiseCircular => self.arc(start, end, di, dj, false),
            InterpolationMode::CounterclockwiseCircular => self.arc(start, end, di, dj, true),
        }
    }

    fn arc(&mut self, start: Vec2, end: Vec2, di: f64, dj: f64, ccw: bool) -> Result<()> {
        let multi_centre = Vec2::new(start.x + di, start.y + dj);
        let multi = match self.quadrant {
            Quadrant::Multi => true,
            Quadrant::Single => false,
            Quadrant::Unset => {
                self.warn("The file draws an arc before choosing a quadrant mode (G74/G75); it was guessed from the arc.".into());
                let r = dist(multi_centre, start);
                (dist(multi_centre, end) - r).abs() <= tolerance(r)
            }
        };
        let centre = if multi { multi_centre } else { single_quadrant_centre(start, end, di.abs(), dj.abs(), ccw) };
        let r = dist(centre, start);
        if !r.is_finite() || r <= 1e-9 {
            if start != end {
                self.warn("An arc with zero radius was read as a straight line.".into());
                self.push(Geom2::Line(start, end))?;
            }
            return Ok(());
        }
        if start == end {
            // A full circle in multi-quadrant mode; a zero-length arc in single-quadrant mode.
            if multi {
                self.push(Geom2::Circle(centre, r))?;
            }
            return Ok(());
        }
        let off = (dist(centre, end) - r).abs();
        if off > tolerance(r) {
            self.warn(format!("An arc's end point is {off:.4} mm off its circle; read anyway."));
        }
        // Geom2 arcs run counter-clockwise, so a clockwise arc runs from its end to its start.
        let (a, b) = if ccw { (start, end) } else { (end, start) };
        self.push(Geom2::Arc { c: centre, a, b })
    }
}

fn unit_mm(u: Unit) -> f64 {
    match u {
        Unit::Millimeters => 1.0,
        Unit::Inches => MM_PER_INCH,
    }
}

/// The text of a `G04` comment block, if this is one.
fn comment_text(w: &str) -> Option<&str> {
    let rest = w.strip_prefix('G')?;
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let code: u32 = rest.get(..digits)?.parse().ok()?;
    (code == 4).then(|| rest.get(digits..).unwrap_or(""))
}

/// The words of a block without whitespace, or `None` when it is not made of known words.
fn split_words(s: &str) -> Option<Words<'_>> {
    let b = s.as_bytes();
    let mut words = Words::default();
    let mut k = 0;
    while let Some(&letter) = b.get(k) {
        k += 1;
        let signed = matches!(letter, b'X' | b'Y' | b'I' | b'J');
        let mut negative = false;
        if signed {
            match b.get(k) {
                Some(b'-') => {
                    negative = true;
                    k += 1;
                }
                Some(b'+') => k += 1,
                _ => {}
            }
        }
        let start = k;
        while b.get(k).is_some_and(u8::is_ascii_digit) {
            k += 1;
        }
        let digits = s.get(start..k)?;
        if digits.is_empty() {
            return None;
        }
        let code = || digits.parse::<u32>().ok();
        match letter {
            b'G' if words.g.len() < 8 => words.g.push(code()?),
            b'D' if words.d.is_none() => words.d = Some(code()?),
            b'M' if words.m.is_none() => words.m = Some(code()?),
            b'X' if words.x.is_none() => words.x = Some((negative, digits)),
            b'Y' if words.y.is_none() => words.y = Some((negative, digits)),
            b'I' if words.i.is_none() => words.i = Some((negative, digits)),
            b'J' if words.j.is_none() => words.j = Some((negative, digits)),
            _ => return None,
        }
    }
    Some(words)
}

/// The centre of a single-quadrant (G74) arc: offsets are unsigned, so of the four centres the
/// one whose arc in this direction spans at most a quadrant, with the end nearest its circle.
fn single_quadrant_centre(start: Vec2, end: Vec2, i: f64, j: f64, ccw: bool) -> Vec2 {
    let mut best: Option<(bool, f64, Vec2)> = None;
    for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
        let c = Vec2::new(start.x + sx * i, start.y + sy * j);
        let sweep = if ccw { ccw_sweep(c, start, end) } else { ccw_sweep(c, end, start) };
        let fits = sweep <= QUADRANT;
        let off = (dist(c, end) - dist(c, start)).abs();
        let better = match best {
            None => true,
            Some((bf, boff, _)) => (fits && !bf) || (fits == bf && off < boff),
        };
        if better {
            best = Some((fits, off, c));
        }
    }
    best.map_or(Vec2::new(start.x + i, start.y + j), |(_, _, c)| c)
}

/// Counter-clockwise angle from `a` to `b` around `c`, in `[0, 2π)`.
fn ccw_sweep(c: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ta = (a.y - c.y).atan2(a.x - c.x);
    let tb = (b.y - c.y).atan2(b.x - c.x);
    (tb - ta).rem_euclid(std::f64::consts::TAU)
}

fn dist(a: Vec2, b: Vec2) -> f64 {
    (b.x - a.x).hypot(b.y - a.y)
}

/// How far an arc's end may be off its circle before a warning.
fn tolerance(r: f64) -> f64 {
    (1e-3 * r).max(1e-3)
}

fn check_range(mm: f64) -> Result<()> {
    if mm.is_finite() && mm.abs() <= MAX_COORD_MM {
        Ok(())
    } else {
        Err(IoError::Invalid("a Gerber coordinate is more than a kilometre from the origin; the file is probably damaged".into()))
    }
}

/// A short, single-line excerpt of file text for a message.
fn quote(s: &str) -> String {
    let mut out: String = s.chars().filter(|c| !c.is_control()).take(MAX_QUOTE).collect();
    if s.chars().count() > MAX_QUOTE {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
#[path = "gerber_tests.rs"]
mod tests;
