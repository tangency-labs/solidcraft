//! Programs as G-code: [`emit`] writes one through a [`Post`], [`parse`] reads the motion
//! back (the subset `emit` writes), which is how the output is checked.

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::path::{ArcPlane, Move, PlaneArc, Toolpath, arc_sweep};
use crate::post::{CommentStyle, Post, Values, fill};
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Most operations in one program.
pub(crate) const MAX_OPERATIONS: usize = 10_000;
/// Most lines [`parse`] reads.
pub(crate) const MAX_LINES: usize = 5_000_000;
/// Longest comment written.
pub(crate) const MAX_COMMENT: usize = 80;
/// Slowest and fastest feed written (mm/min).
pub(crate) const MIN_FEED: f64 = 1.0e-3;
pub(crate) const MAX_FEED: f64 = 1.0e6;
/// Largest feed word written (in the post's units a minute, or degrees a minute).
pub(crate) const MAX_FEED_WORD: f64 = 1.0e7;

/// Whether `feed` (mm/min) is one a program may be written with: [`MIN_FEED`] to [`MAX_FEED`].
pub(crate) fn feed_in_range(feed: f64) -> bool {
    (MIN_FEED..=MAX_FEED).contains(&feed)
}

/// Whether a written `F` value reads back as a usable feed: above zero (not rounded away to
/// `F0`) and at most [`MAX_FEED_WORD`] when `per_minute`.
pub(crate) fn feed_word_ok(word: &str, per_minute: bool) -> bool {
    word.trim_end_matches('.').parse::<f64>().is_ok_and(|v| v > 0.0 && (!per_minute || v <= MAX_FEED_WORD))
}

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
pub(crate) fn number(v: f64, decimals: u8) -> String {
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
pub(crate) fn plain(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_alphanumeric() || " ._-+/:=,#".contains(*c)).take(MAX_COMMENT).collect::<String>().trim().to_string()
}

/// A number as `post` wants it: at most `decimals` decimals, with a decimal point even when
/// whole if the post says so.
pub(crate) fn post_number(post: &Post, v: f64, decimals: u8) -> String {
    let s = number(v, decimals);
    if post.trailing_decimal && !s.contains('.') { format!("{s}.") } else { s }
}

/// The words of a block joined by single spaces, empty words left out.
pub(crate) fn join(words: &[String]) -> String {
    words.iter().flat_map(|w| w.split_whitespace()).collect::<Vec<_>>().join(" ")
}

/// The tool list line of [`Post::tool_list`] for `tool`, whose operations go down to `zmin`
/// (mm; `None` to leave it out).
pub(crate) fn tool_list_line(post: &Post, tool: &Tool, zmin: Option<f64>) -> String {
    let mm = post.units.mm();
    let corner = match tool.kind {
        ToolKind::BallEndmill => tool.diameter / 2.0,
        ToolKind::Chamfer => 0.0,
        _ => tool.corner_radius.unwrap_or(0.0),
    };
    let mut line =
        format!("T{} D={} CR={}", tool.number, post_number(post, tool.diameter / mm, post.decimals), post_number(post, corner / mm, post.decimals));
    if let Some(z) = zmin.filter(|z| z.is_finite()) {
        line.push_str(&format!(" - ZMIN={}", post_number(post, z / mm, post.decimals)));
    }
    line.push_str(&format!(" - {}", tool.kind.label()));
    line
}

/// The comment of an operation: its name, and unless the post wants the name alone, its tool.
pub(crate) fn operation_comment(post: &Post, name: &str, tool: &Tool) -> String {
    if post.operation_name_only {
        name.to_string()
    } else {
        format!("{}: T{} D{} {}", name, tool.number, number(tool.diameter, 3), tool.kind.label())
    }
}

/// Each tool in the order it is first used, with the lowest Z of its operations.
fn tools_used(program: &Program) -> Vec<(&Tool, Option<f64>)> {
    let mut out: Vec<(&Tool, Option<f64>)> = Vec::new();
    for op in &program.operations {
        let low = op.path.moves.iter().filter_map(Move::target).map(|p| p.z).filter(|z| z.is_finite()).reduce(f64::min);
        match out.iter_mut().find(|(t, _)| t.number == op.tool.number) {
            Some((_, z)) => *z = (*z).into_iter().chain(low).reduce(f64::min),
            None => out.push((&op.tool, low)),
        }
    }
    out
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
    /// The tool's height, when known (it may be while X and Y are not).
    height: Option<f64>,
    /// Whether the tool is at the machine's top after the post's safe retract, nothing moved since.
    top: bool,
    feed: Option<String>,
    /// The motion mode in force as written (`G0` to `G3`), when known.
    mode: Option<u8>,
    /// The tool whose length offset goes with the next Z word ([`Post::tool_length_on_first_z`]).
    length_offset: Option<u32>,
    /// The tool in the spindle.
    tool: Option<u32>,
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
        let text = self.post.comment_text(text);
        match self.post.comments {
            CommentStyle::Parentheses => self.out.push_str(&format!("({text})\n")),
            CommentStyle::Semicolon => self.out.push_str(&format!("; {text}\n")),
            CommentStyle::None => {}
        }
    }

    /// A blank line, if the post spaces its operations out.
    fn blank(&mut self) {
        if self.post.blank_lines {
            self.out.push('\n');
        }
    }

    fn template(&mut self, lines: &[String], values: &Values) -> Result<()> {
        for line in lines {
            let text = fill(line, values)?;
            self.block(&text);
        }
        // A template may hold any motion code: the next motion states its own.
        if !lines.is_empty() {
            self.mode = None;
        }
        Ok(())
    }

    /// The post's safe retract: afterwards the tool is at the machine's top, and the next Z
    /// move sets the tool length offset again if the post sets it with the first Z. Not
    /// written again while the tool is still up there.
    fn safe_retract(&mut self, lines: &[String], values: &Values) -> Result<()> {
        if self.top {
            return Ok(());
        }
        self.template(lines, values)?;
        self.forget();
        self.top = true;
        if self.post.tool_length_on_first_z {
            self.length_offset = self.tool;
        }
        Ok(())
    }

    /// A number as the controller wants it: with a decimal point even when whole, if the post says so.
    fn value(&self, v: f64, decimals: u8) -> String {
        post_number(self.post, v, decimals)
    }

    fn coord(&self, mm: f64) -> String {
        self.value(mm / self.post.units.mm(), self.post.decimals)
    }

    /// Forget where the tool is (after a tool change, which may move it): the next move states
    /// every axis, and the next rapid rises before it moves across.
    fn forget(&mut self) {
        (self.x, self.y, self.z, self.pos, self.height, self.feed, self.top) = (None, None, None, None, None, None, false);
    }

    /// The word of motion code `n` (0–3): `G1`, or `G01` for a post that pads its codes; nothing
    /// for a modal post already in that mode.
    fn motion(&mut self, n: u8) -> String {
        if self.post.modal_motion && self.mode == Some(n) {
            return String::new();
        }
        self.mode = Some(n);
        if self.post.two_digit_codes { format!("G{n:02}") } else { format!("G{n}") }
    }

    /// A `G0`/`G1` block for the axes among `xy` and `z` that change; nothing if none does.
    fn linear(&mut self, code: u8, to: Vec3, xy: bool, z: bool, feed: Option<f64>) {
        let (nx, ny, nz) = (self.coord(to.x), self.coord(to.y), self.coord(to.z));
        let mut words = Vec::new();
        if xy && self.x.as_ref() != Some(&nx) {
            words.push(format!("X{nx}"));
            self.x = Some(nx);
        }
        if xy && self.y.as_ref() != Some(&ny) {
            words.push(format!("Y{ny}"));
            self.y = Some(ny);
        }
        let mut offset = None;
        if z && self.z.as_ref() != Some(&nz) {
            offset = self.length_offset.take();
            words.push(format!("Z{nz}"));
            self.z = Some(nz);
        }
        if words.is_empty() {
            return;
        }
        self.top = false;
        let feed = self.feed_word(feed);
        let code = self.motion(code);
        let mut block = vec![code];
        if let Some(t) = offset {
            block.push("G43".into());
            block.extend(words);
            block.push(format!("H{t}"));
        } else {
            block.extend(words);
        }
        block.push(feed);
        self.block(&join(&block));
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

    /// A rapid, never a diagonal: up first when rising, across first when descending. Where the
    /// tool's height is not known (the program's start, after a tool change) it first rises to
    /// `rise` (or the rapid's own height, if higher), so it never moves across at a height it
    /// does not know; at the machine's top it goes across first.
    fn rapid(&mut self, to: Vec3, rise: Option<f64>) {
        let up_first = match self.height {
            Some(z) => to.z > z,
            None if self.top => false,
            None => {
                let up = rise.map_or(to.z, |r| r.max(to.z));
                self.linear(0, Vec3::new(to.x, to.y, up), false, true, None);
                false
            }
        };
        if up_first {
            self.linear(0, to, false, true, None);
            self.linear(0, to, true, false, None);
        } else {
            self.linear(0, to, true, false, None);
            self.linear(0, to, false, true, None);
        }
        (self.pos, self.height) = (Some(to), Some(to.z));
    }

    fn feed(&mut self, to: Vec3, feed: f64) {
        self.linear(1, to, true, true, Some(feed));
        (self.pos, self.height) = (Some(to), Some(to.z));
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
        let mut words = vec![self.motion(if ccw { 3 } else { 2 })];
        // A modal post leaves out an end coordinate that does not change.
        if !self.post.modal_motion || self.x.as_ref() != Some(&nx) {
            words.push(format!("X{nx}"));
        }
        if !self.post.modal_motion || self.y.as_ref() != Some(&ny) {
            words.push(format!("Y{ny}"));
        }
        if self.z.as_ref() != Some(&nz) {
            words.push(format!("Z{nz}"));
        }
        words.push(format!("I{}", self.coord(center.x - from.x)));
        words.push(format!("J{}", self.coord(center.y - from.y)));
        words.push(self.feed_word(Some(feed)));
        (self.x, self.y, self.z, self.pos, self.height, self.top) = (Some(nx), Some(ny), Some(nz), Some(to), Some(to.z), false);
        self.block(&join(&words));
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
    let mut w = Writer {
        post,
        out: String::new(),
        line: post.line_numbers.map(|n| n.start),
        x: None,
        y: None,
        z: None,
        pos: None,
        height: None,
        top: false,
        feed: None,
        mode: None,
        length_offset: None,
        tool: None,
    };
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
    if post.tool_list {
        for (tool, zmin) in tools_used(program) {
            w.comment(&tool_list_line(post, tool, zmin));
        }
    }
    w.template(&post.header, &values)?;
    if let Some(lines) = post.safe_retract_at(|r| r.at_start) {
        w.safe_retract(lines, &values)?;
    }
    // Where the tool's height is not known, the first rapid rises to the program's highest.
    let rise = program
        .operations
        .iter()
        .flat_map(|op| &op.path.moves)
        .filter_map(|m| match m {
            Move::Rapid { to } if to.z.is_finite() => Some(to.z),
            _ => None,
        })
        .reduce(f64::max);
    let mut tool: Option<u32> = None;
    let mut rpm: Option<String> = None;
    let mut coolant = false;
    for op in &program.operations {
        op.tool.validate()?;
        if !(op.spindle_rpm > 0.0) || !(op.spindle_rpm <= 1.0e6) {
            return invalid(format!("operation `{}`: the spindle speed must be above 0", plain(&op.name)));
        }
        let new_tool = tool != Some(op.tool.number);
        // The spindle stops before the tool rises for a later tool change.
        if new_tool
            && tool.is_some()
            && let Some(lines) = post.safe_retract_at(|r| r.at_tool_change)
        {
            w.template(&post.spindle_off, &values)?;
            w.safe_retract(lines, &values)?;
        }
        let speed = number(op.spindle_rpm, 0);
        values.tool = op.tool.number.to_string();
        values.diameter = number(op.tool.diameter / post.units.mm(), post.decimals);
        values.rpm = speed.clone();
        w.blank();
        w.comment(&operation_comment(post, &op.name, &op.tool));
        if new_tool {
            if tool.is_some() || post.tool_change_first {
                // Coolant off while the tool is changed; the operation turns it back on.
                if coolant {
                    w.template(&post.coolant_off, &values)?;
                    coolant = false;
                }
                w.template(&post.tool_change, &values)?;
                if tool.is_some() && post.stops_at_tool_change() {
                    w.template(&["M5".to_string()], &values)?;
                    w.comment(&format!("change to tool {} diameter {}, set its length, then cycle start", values.tool, values.diameter));
                    w.template(&["M0".to_string()], &values)?;
                }
            }
            // A tool change stops the spindle and may move the tool anywhere, except that one
            // made at the machine's top leaves it there.
            rpm = None;
            tool = Some(op.tool.number);
            let top = w.top;
            w.forget();
            w.top = top;
            w.tool = tool;
            if post.tool_length_on_first_z {
                w.length_offset = tool;
            }
        }
        if rpm.as_ref() != Some(&speed) {
            w.template(&post.spindle_on, &values)?;
            rpm = Some(speed);
        }
        w.template(&post.operation_start, &values)?;
        if op.coolant != coolant {
            w.template(if op.coolant { &post.coolant_on } else { &post.coolant_off }, &values)?;
            coolant = op.coolant;
        }
        // With the same tool the tool is still where the last operation left it; only the feed
        // is stated again.
        w.feed = None;
        for m in &op.path.moves {
            if m.target().is_some_and(|to| !to.is_finite()) {
                return Err(CamError::Unsafe(format!("operation `{}` has a coordinate that is not a finite number", plain(&op.name))));
            }
            match *m {
                Move::Rapid { to } => w.rapid(to, rise),
                Move::Feed { to, feed } | Move::Arc { to, feed, .. } | Move::PlaneArc { to, feed, .. }
                    if !feed_in_range(feed) || !feed_word_ok(&w.value(feed / post.units.mm(), post.feed_decimals), true) =>
                {
                    let _ = to;
                    return Err(CamError::Unsafe(format!(
                        "operation `{}` has a feed that is not {MIN_FEED} to {MAX_FEED} mm/min or that would be written as F0",
                        plain(&op.name)
                    )));
                }
                Move::Feed { to, feed } => w.feed(to, feed),
                Move::Arc { to, center, ccw, feed } => w.arc(to, center, ccw, feed)?,
                // This writer keeps to G17: a vertical arc goes as lines within the arc tolerance.
                Move::PlaneArc { to, center, plane, ccw, feed } => match w.pos {
                    Some(from) => {
                        for p in (PlaneArc { from, to, center, plane, ccw }).points(post.arc_tolerance) {
                            w.feed(p, feed);
                        }
                    }
                    None => return Err(CamError::Unsafe("an arc before the first positioning move".into())),
                },
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
        w.template(&post.operation_end, &values)?;
    }
    w.blank();
    if coolant {
        w.template(&post.coolant_off, &values)?;
    }
    w.template(&post.spindle_off, &values)?;
    if let Some(lines) = post.safe_retract_at(|r| r.at_end) {
        w.safe_retract(lines, &values)?;
    }
    w.template(&post.footer, &values)?;
    Ok(w.out)
}

/// The words of one line: `(letter, value)`.
pub(crate) fn words(line: &str, n: usize) -> Result<Vec<(char, f64)>> {
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
/// `G4 P`, `G20`/`G21`, arc centres `I J` from the arc's start (`G91.1`; `G90.1` is refused).
/// A return to the machine's home (`G28`/`G30`, as in `G28 G91 Z0.`) makes the axes it names
/// unknown until they are written again; any other move in incremental coordinates (`G91`) is
/// refused. Everything else (spindle, tools, coolant, offsets) is skipped. The result is in
/// millimetres and mm/min.
pub fn parse(text: &str) -> Result<Vec<Move>> {
    let mut units = 1.0;
    let mut mode: Option<u8> = None;
    let mut feed: Option<f64> = None;
    let mut incremental = false;
    let mut plane = ArcPlane::Xy;
    let (mut x, mut y, mut z): (Option<f64>, Option<f64>, Option<f64>) = (None, None, None);
    let mut pos: Option<Vec3> = None;
    let mut moves = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if n > MAX_LINES {
            return Err(CamError::TooLarge(format!("more than {MAX_LINES} lines")));
        }
        let ws = words(line, n)?;
        let (mut dwell, mut home, mut g91) = (false, false, false);
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
                170 => plane = ArcPlane::Xy,
                180 => plane = ArcPlane::Zx,
                190 => plane = ArcPlane::Yz,
                200 => units = 25.4,
                210 => units = 1.0,
                280 | 300 => home = true,
                900 => incremental = false,
                910 => (incremental, g91) = (true, true),
                901 => return Err(CamError::Parse(n, "not supported yet: absolute arc centres (G90.1)".into())),
                _ => {}
            }
        }
        if g91 && !home {
            return Err(CamError::Parse(n, "not supported yet: incremental coordinates (G91) other than for a return home (G28 G91)".into()));
        }
        let word = |letter: char| ws.iter().find(|(c, _)| *c == letter).map(|(_, v)| *v);
        if home {
            for (slot, letter) in [(&mut x, 'X'), (&mut y, 'Y'), (&mut z, 'Z')] {
                if word(letter).is_some() {
                    *slot = None;
                }
            }
            pos = None;
            continue;
        }
        if incremental && ['X', 'Y', 'Z', 'I', 'J', 'K'].into_iter().any(|l| word(l).is_some()) {
            return Err(CamError::Parse(n, "not supported yet: incremental coordinates (G91)".into()));
        }
        if dwell {
            // `G4 P` in seconds as this crate's own posts write it, or `G04 X` in seconds.
            moves.push(Move::Dwell { seconds: word('X').or(word('P')).unwrap_or(0.0).max(0.0) });
            continue;
        }
        if let Some(f) = word('F') {
            feed = Some(f * units);
        }
        let (tx, ty, tz, ti, tj, tk) = (word('X'), word('Y'), word('Z'), word('I'), word('J'), word('K'));
        if tx.is_none() && ty.is_none() && tz.is_none() && ti.is_none() && tj.is_none() && tk.is_none() {
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
                    let offsets = [ti, tj, tk];
                    let (u, v, _) = plane.axes();
                    let given = |i: usize| offsets.get(i).copied().flatten();
                    if given(u).is_none() && given(v).is_none() {
                        let words = match plane {
                            ArcPlane::Xy => "I and J",
                            ArcPlane::Zx => "I and K (G18)",
                            ArcPlane::Yz => "J and K (G19)",
                        };
                        return Err(CamError::Parse(n, format!("not supported yet: arcs without {words}")));
                    }
                    let off = |i: usize| if i == u || i == v { given(i).unwrap_or(0.0) * units } else { 0.0 };
                    if plane == ArcPlane::Xy {
                        let center = Vec2::new(from.x + off(0), from.y + off(1));
                        moves.push(Move::Arc { to, center, ccw: mode == 3, feed });
                    } else {
                        let center = from + Vec3::new(off(0), off(1), off(2));
                        moves.push(Move::PlaneArc { to, center, plane, ccw: mode == 3, feed });
                    }
                }
            }
        }
        pos = Some(to);
    }
    Ok(moves)
}
