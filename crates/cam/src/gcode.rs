//! Programs as G-code: [`emit`] writes one through a [`Post`], [`parse`] reads the motion
//! back (the subset `emit` writes), which is how the output is checked.

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Vec2, Vec3};

use crate::path::{Move, Toolpath, arc_sweep};
use crate::post::{CommentStyle, Post, Values, fill};
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// Most operations in one program.
const MAX_OPERATIONS: usize = 10_000;
/// Most lines [`parse`] reads.
const MAX_LINES: usize = 5_000_000;
/// Longest comment written.
const MAX_COMMENT: usize = 80;

/// One operation of a program: a tool, how it turns, and where it goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub name: String,
    pub tool: Tool,
    /// Spindle speed (rev/min), clockwise.
    pub spindle_rpm: f64,
    pub coolant: bool,
    pub path: Toolpath,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub name: String,
    pub operations: Vec<Operation>,
    /// Work offset 1 to 6 (`G54` to `G59`); left out, the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_offset: Option<u8>,
}

/// The code of work offset `n` (1 to 6): `G54` to `G59`.
pub fn work_offset_code(n: u8) -> Result<String> {
    if !(1..=6).contains(&n) {
        return invalid("the work offset must be 1 to 6 (G54 to G59)");
    }
    Ok(format!("G{}", 53 + u32::from(n)))
}

/// `v` with at most `decimals` decimals and no trailing zeros.
fn number(v: f64, decimals: u8) -> String {
    let mut s = format!("{:.*}", usize::from(decimals), v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" { "0".into() } else { s }
}

/// Text that is safe inside any comment style.
fn plain(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_alphanumeric() || " ._-+/:=,#".contains(*c)).take(MAX_COMMENT).collect::<String>().trim().to_string()
}

struct Writer<'a> {
    post: &'a Post,
    out: String,
    line: Option<u32>,
    /// The position as written (what the controller believes) and exactly.
    x: Option<String>,
    y: Option<String>,
    z: Option<String>,
    pos: Option<Vec3>,
    feed: Option<String>,
}

impl Writer<'_> {
    fn block(&mut self, text: &str) {
        if let (Some(n), Some(numbering)) = (self.line, self.post.line_numbers) {
            self.out.push_str(&format!("N{n} "));
            self.line = Some(n.saturating_add(numbering.step));
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn comment(&mut self, text: &str) {
        let text = plain(text);
        match self.post.comments {
            CommentStyle::Parentheses => self.out.push_str(&format!("({text})\n")),
            CommentStyle::Semicolon => self.out.push_str(&format!("; {text}\n")),
            CommentStyle::None => {}
        }
    }

    fn template(&mut self, lines: &[String], values: &Values) -> Result<()> {
        for line in lines {
            let text = fill(line, values)?;
            self.block(&text);
        }
        Ok(())
    }

    /// A number as the controller wants it: with a decimal point even when whole, if the post says so.
    fn value(&self, v: f64, decimals: u8) -> String {
        let s = number(v, decimals);
        if self.post.trailing_decimal && !s.contains('.') { format!("{s}.") } else { s }
    }

    fn coord(&self, mm: f64) -> String {
        self.value(mm / self.post.units.mm(), self.post.decimals)
    }

    /// Forget where the tool is: the next move states every axis.
    fn reset(&mut self) {
        (self.x, self.y, self.z, self.pos, self.feed) = (None, None, None, None, None);
    }

    /// A `G0`/`G1` block for the axes among `xy` and `z` that change; nothing if none does.
    /// The word of motion code `n` (0–3): `G1`, or `G01` for a post that pads its codes.
    fn motion(&self, n: u8) -> String {
        if self.post.two_digit_codes { format!("G{n:02}") } else { format!("G{n}") }
    }

    fn linear(&mut self, code: u8, to: Vec3, xy: bool, z: bool, feed: Option<f64>) {
        let (nx, ny, nz) = (self.coord(to.x), self.coord(to.y), self.coord(to.z));
        let mut words = String::new();
        if xy && self.x.as_ref() != Some(&nx) {
            words.push_str(&format!(" X{nx}"));
            self.x = Some(nx);
        }
        if xy && self.y.as_ref() != Some(&ny) {
            words.push_str(&format!(" Y{ny}"));
            self.y = Some(ny);
        }
        if z && self.z.as_ref() != Some(&nz) {
            words.push_str(&format!(" Z{nz}"));
            self.z = Some(nz);
        }
        if words.is_empty() {
            return;
        }
        let feed = self.feed_word(feed);
        let code = self.motion(code);
        self.block(&format!("{code}{words}{feed}"));
    }

    /// ` F…` when the feed changes.
    fn feed_word(&mut self, feed: Option<f64>) -> String {
        let Some(feed) = feed else { return String::new() };
        let f = self.value(feed / self.post.units.mm(), self.post.feed_decimals);
        if self.feed.as_ref() == Some(&f) {
            return String::new();
        }
        self.feed = Some(f.clone());
        format!(" F{f}")
    }

    fn rapid(&mut self, to: Vec3) {
        // Never a diagonal rapid: up first when rising, across first when descending.
        if self.pos.is_some_and(|p| to.z > p.z) {
            self.linear(0, to, false, true, None);
            self.linear(0, to, true, false, None);
        } else {
            self.linear(0, to, true, false, None);
            self.linear(0, to, false, true, None);
        }
        self.pos = Some(to);
    }

    fn feed(&mut self, to: Vec3, feed: f64) {
        self.linear(1, to, true, true, Some(feed));
        self.pos = Some(to);
    }

    fn arc(&mut self, to: Vec3, center: Vec2, ccw: bool, feed: f64) -> Result<()> {
        let Some(from) = self.pos else { return Err(CamError::Unsafe("an arc before the first positioning move".into())) };
        let (nx, ny) = (self.coord(to.x), self.coord(to.y));
        // An arc whose end is written the same as its start would be read as a full circle.
        if self.x.as_ref() == Some(&nx) && self.y.as_ref() == Some(&ny) {
            self.feed(to, feed);
            return Ok(());
        }
        // A helix on a controller that only knows flat arcs goes out as lines too.
        let helix = self.z.as_ref() != Some(&self.coord(to.z));
        if !self.post.arcs || (helix && !self.post.helical_arcs) {
            let start = Vec2::new(from.x, from.y) - center;
            let (r, sweep) = (start.len(), arc_sweep(from, to, center, ccw));
            let tol = self.post.arc_tolerance;
            let step = if tol < r { 2.0 * (1.0 - tol / r).acos() } else { std::f64::consts::FRAC_PI_4 };
            let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 4096);
            for i in 1..n {
                let t = i as f64 / n as f64;
                let p = center + Vec2::from_angle(start.angle() + sweep * t) * r;
                self.feed(p.extend(from.z + (to.z - from.z) * t), feed);
            }
            self.feed(to, feed);
            return Ok(());
        }
        let nz = self.coord(to.z);
        let mut words = format!("{} X{nx} Y{ny}", self.motion(if ccw { 3 } else { 2 }));
        if self.z.as_ref() != Some(&nz) {
            words.push_str(&format!(" Z{nz}"));
        }
        words.push_str(&format!(" I{} J{}", self.coord(center.x - from.x), self.coord(center.y - from.y)));
        words.push_str(&self.feed_word(Some(feed)));
        (self.x, self.y, self.z, self.pos) = (Some(nx), Some(ny), Some(nz), Some(to));
        self.block(&words);
        Ok(())
    }
}

/// Writes `program` for the controller `post` describes.
pub fn emit(program: &Program, post: &Post) -> Result<String> {
    post.validate()?;
    if program.operations.is_empty() {
        return invalid("the program has no operation");
    }
    if program.operations.len() > MAX_OPERATIONS {
        return Err(CamError::TooLarge(format!("more than {MAX_OPERATIONS} operations")));
    }
    let mut w = Writer { post, out: String::new(), line: post.line_numbers.map(|n| n.start), x: None, y: None, z: None, pos: None, feed: None };
    let offset = program.work_offset.unwrap_or(1);
    // A post that leaves the offset to the operator can only mean the first one.
    if offset != 1 && !post.writes_work_offset() {
        return invalid(format!(
            "post `{}` writes no work offset, so the program would run in whatever offset the machine is in rather than {}",
            plain(&post.name),
            work_offset_code(offset)?
        ));
    }
    let mut values =
        Values { program: plain(&program.name), units: post.units.code().into(), work_offset: work_offset_code(offset)?, ..Values::default() };
    w.comment(&program.name);
    w.template(&post.header, &values)?;
    let mut tool: Option<u32> = None;
    let mut rpm: Option<String> = None;
    let mut coolant = false;
    for op in &program.operations {
        op.tool.validate()?;
        if !(op.spindle_rpm > 0.0) || !(op.spindle_rpm <= 1.0e6) {
            return invalid(format!("operation `{}`: the spindle speed must be above 0", plain(&op.name)));
        }
        let speed = number(op.spindle_rpm, 0);
        values.tool = op.tool.number.to_string();
        values.diameter = number(op.tool.diameter / post.units.mm(), post.decimals);
        values.rpm = speed.clone();
        w.comment(&format!("{}: T{} D{} {}", op.name, op.tool.number, number(op.tool.diameter, 3), op.tool.kind.label()));
        if tool != Some(op.tool.number) {
            if tool.is_some() || post.tool_change_first {
                w.template(&post.tool_change, &values)?;
            }
            // A tool change stops the spindle.
            rpm = None;
            tool = Some(op.tool.number);
        }
        if rpm.as_ref() != Some(&speed) {
            w.template(&post.spindle_on, &values)?;
            rpm = Some(speed);
        }
        if op.coolant != coolant {
            w.template(if op.coolant { &post.coolant_on } else { &post.coolant_off }, &values)?;
            coolant = op.coolant;
        }
        w.reset();
        for m in &op.path.moves {
            if m.target().is_some_and(|to| !to.is_finite()) {
                return Err(CamError::Unsafe(format!("operation `{}` has a coordinate that is not a finite number", plain(&op.name))));
            }
            match *m {
                Move::Rapid { to } => w.rapid(to),
                Move::Feed { to, feed } | Move::Arc { to, feed, .. } if !(feed > 0.0) || !feed.is_finite() => {
                    let _ = to;
                    return Err(CamError::Unsafe(format!("operation `{}` has a feed that is not above 0", plain(&op.name))));
                }
                Move::Feed { to, feed } => w.feed(to, feed),
                Move::Arc { to, center, ccw, feed } => w.arc(to, center, ccw, feed)?,
                Move::Dwell { seconds } => {
                    if !(seconds >= 0.0) || !seconds.is_finite() {
                        return Err(CamError::Unsafe("a dwell must be 0 seconds or longer".into()));
                    }
                    values.seconds = w.value(seconds, 3);
                    values.milliseconds = number(seconds * 1000.0, 0);
                    let dwell = fill(&post.dwell, &values)?;
                    w.block(&dwell);
                }
            }
        }
    }
    if coolant {
        w.template(&post.coolant_off, &values)?;
    }
    w.template(&post.spindle_off, &values)?;
    w.template(&post.footer, &values)?;
    Ok(w.out)
}

/// The words of one line: `(letter, value)`.
fn words(line: &str, n: usize) -> Result<Vec<(char, f64)>> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            ';' | '%' => break,
            '(' => {
                for d in chars.by_ref() {
                    if d == ')' {
                        break;
                    }
                }
            }
            c if c.is_ascii_alphabetic() => {
                let mut text = String::new();
                while let Some(d) = chars.peek().copied().filter(|d| d.is_ascii_digit() || "+-.".contains(*d)) {
                    text.push(d);
                    chars.next();
                }
                let value: f64 = text.parse().map_err(|_| CamError::Parse(n, format!("`{c}{text}` is not a word with a number")))?;
                if !value.is_finite() {
                    return Err(CamError::Parse(n, format!("`{c}{text}` is not a finite number")));
                }
                out.push((c.to_ascii_uppercase(), value));
            }
            other => return Err(CamError::Parse(n, format!("unexpected `{other}`"))),
        }
    }
    Ok(out)
}

/// Reads the motion of a G-code program in absolute coordinates: `G0`–`G3` with `X Y Z I J F`,
/// `G4 P`, `G20`/`G21`. Everything else (spindle, tools, coolant, offsets) is skipped. The
/// result is in millimetres and mm/min.
pub fn parse(text: &str) -> Result<Vec<Move>> {
    let mut units = 1.0;
    let mut mode: Option<u8> = None;
    let mut feed: Option<f64> = None;
    let (mut x, mut y, mut z): (Option<f64>, Option<f64>, Option<f64>) = (None, None, None);
    let mut pos: Option<Vec3> = None;
    let mut moves = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if n > MAX_LINES {
            return Err(CamError::TooLarge(format!("more than {MAX_LINES} lines")));
        }
        let ws = words(line, n)?;
        let mut dwell = false;
        for &(c, v) in &ws {
            if c != 'G' {
                continue;
            }
            match (v * 10.0).round() as i64 {
                0 => mode = Some(0),
                10 => mode = Some(1),
                20 => mode = Some(2),
                30 => mode = Some(3),
                40 => dwell = true,
                200 => units = 25.4,
                210 => units = 1.0,
                910 => return Err(CamError::Parse(n, "not supported yet: incremental coordinates (G91)".into())),
                _ => {}
            }
        }
        let word = |letter: char| ws.iter().find(|(c, _)| *c == letter).map(|(_, v)| *v);
        if dwell {
            // `G4 P` in seconds as this crate's own posts write it, or `G04 X` in seconds.
            moves.push(Move::Dwell { seconds: word('X').or(word('P')).unwrap_or(0.0).max(0.0) });
            continue;
        }
        if let Some(f) = word('F') {
            feed = Some(f * units);
        }
        let (tx, ty, tz, ti, tj) = (word('X'), word('Y'), word('Z'), word('I'), word('J'));
        if tx.is_none() && ty.is_none() && tz.is_none() && ti.is_none() && tj.is_none() {
            continue;
        }
        let Some(mode) = mode else { return Err(CamError::Parse(n, "coordinates before any G0, G1, G2 or G3".into())) };
        x = tx.map(|v| v * units).or(x);
        y = ty.map(|v| v * units).or(y);
        z = tz.map(|v| v * units).or(z);
        let (Some(px), Some(py), Some(pz)) = (x, y, z) else { continue };
        let to = Vec3::new(px, py, pz);
        match mode {
            0 => moves.push(Move::Rapid { to }),
            _ => {
                let Some(feed) = feed.filter(|f| *f > 0.0) else { return Err(CamError::Parse(n, "a cutting move without a feed rate".into())) };
                if mode == 1 {
                    moves.push(Move::Feed { to, feed });
                } else {
                    let Some(from) = pos else { return Err(CamError::Parse(n, "an arc before the position is known".into())) };
                    if ti.is_none() && tj.is_none() {
                        return Err(CamError::Parse(n, "not supported yet: arcs without I and J".into()));
                    }
                    let center = Vec2::new(from.x + ti.unwrap_or(0.0) * units, from.y + tj.unwrap_or(0.0) * units);
                    moves.push(Move::Arc { to, center, ccw: mode == 3, feed });
                }
            }
        }
        pos = Some(to);
    }
    Ok(moves)
}
