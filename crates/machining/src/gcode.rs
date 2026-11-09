//! G-code: a streaming parser for the v1 subset, against a code table. Never panics: anything it
//! cannot do becomes a [`Diag`] for its line, and the rest of the program still parses.
//!
//! The position is known per axis. After G28 or G30 the named axes (all of them when none is
//! named) are unknown until a move gives them a value. A move is emitted once all three axes are
//! known, and the first such move after an unknown position is a [`MoveKind::Jump`] (it sets the
//! start of the next path and cuts nothing).

use crate::sweep::MAX_ARC_TURNS;
use crate::toolpath::{Diag, Interp, Move, MoveKind, NO_TOOL, Program};

pub const MAX_INPUT_BYTES: usize = 64 << 20;
pub const MAX_LINE_BYTES: usize = 4096;
/// About 300 MB of moves at most, so a program fits the web build's memory.
pub const MAX_MOVES: usize = 2_000_000;
pub const MAX_WORDS: usize = 64;
const MAX_DIAGS: usize = 1000;
const MAX_NUMBER_CHARS: usize = 32;
/// Coordinates, feeds and speeds above this (mm) are rejected, so squares stay finite.
const MAX_VALUE: f64 = 1e9;
const MAX_POSITION: f64 = 1e10;

/// The code table: which G and M numbers mean what. G codes are stored times ten (G90.1 is 901),
/// so every code is an integer. The defaults are the generic 3-axis table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeTable {
    pub rapid: u32,
    pub linear: u32,
    pub arc_cw: u32,
    pub arc_ccw: u32,
    pub plane_xy: u32,
    pub plane_zx: u32,
    pub plane_yz: u32,
    pub inch: u32,
    pub mm: u32,
    pub absolute: u32,
    pub incremental: u32,
    pub arc_centre_absolute: u32,
    pub arc_centre_incremental: u32,
    pub work_offsets: [u32; 6],
    pub machine_coords: u32,
    pub dwell: u32,
    pub cutter_comp_off: u32,
    pub cutter_comp_left: u32,
    pub cutter_comp_right: u32,
    pub tool_length_on: u32,
    pub tool_length_off: u32,
    pub cancel_cycle: u32,
    pub home: [u32; 2],
    pub set_offsets: u32,
    /// Frame-changing codes, each with the transform it turns on (`true`) or off (`false`). While a
    /// transform is on, moves are errors. G52 has no cancel here: it stays on until the program ends.
    pub frame: Vec<(u32, FrameKind, bool)>,
    pub cycle_mode: [u32; 2],
    /// Modal codes that change no motion (feed mode, path control, cutter modes): accepted silently.
    pub ignored: Vec<u32>,
    /// A `/` at the start of a line skips the line when true; when false the slash is ignored.
    pub block_delete: bool,
}

impl Default for CodeTable {
    fn default() -> Self {
        CodeTable {
            rapid: 0,
            linear: 10,
            arc_cw: 20,
            arc_ccw: 30,
            plane_xy: 170,
            plane_zx: 180,
            plane_yz: 190,
            inch: 200,
            mm: 210,
            absolute: 900,
            incremental: 910,
            arc_centre_absolute: 901,
            arc_centre_incremental: 911,
            work_offsets: [540, 550, 560, 570, 580, 590],
            machine_coords: 530,
            dwell: 40,
            cutter_comp_off: 400,
            cutter_comp_left: 410,
            cutter_comp_right: 420,
            tool_length_on: 430,
            tool_length_off: 490,
            cancel_cycle: 800,
            home: [280, 300],
            set_offsets: 100,
            frame: vec![
                (680, FrameKind::Rotate, true),
                (690, FrameKind::Rotate, false),
                (160, FrameKind::Polar, true),
                (150, FrameKind::Polar, false),
                (510, FrameKind::Scale, true),
                (500, FrameKind::Scale, false),
                (520, FrameKind::Local, true),
                (920, FrameKind::Offset, true),
                (921, FrameKind::Offset, false),
                (922, FrameKind::Offset, false),
                (923, FrameKind::Offset, true),
            ],
            cycle_mode: [980, 990],
            // G94 feed per minute, G95 per revolution, G64 path blending, G61 and G61.1 exact stop
            // and exact path, and G67 the macro call cancel.
            ignored: vec![940, 950, 640, 610, 611, 670],
            block_delete: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Motion {
    Rapid,
    Linear,
    ArcCw,
    ArcCcw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plane {
    Xy,
    Zx,
    Yz,
}

/// The coordinate-frame transforms a frame code can turn on: rotation (G68), polar (G16), scaling
/// (G51), the local shift (G52) and the G92 offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    Rotate,
    Polar,
    Scale,
    Local,
    Offset,
}

impl FrameKind {
    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GKind {
    Motion(Motion),
    Plane(Plane),
    Inch,
    Mm,
    Absolute,
    Incremental,
    ArcAbsolute,
    ArcIncremental,
    Offset,
    Machine,
    Dwell,
    CutterOff,
    CutterComp,
    ToolLengthOn,
    ToolLengthOff,
    Home,
    Canned,
    CancelCycle,
    SetOffsets,
    /// A frame-changing code (see [`CodeTable::frame`]): the transform, and whether it turns on.
    Frame(FrameKind, bool),
    /// A modal code that changes no motion.
    Ignored,
    Unknown,
}

impl CodeTable {
    fn classify(&self, code: u32) -> GKind {
        if code == self.rapid {
            GKind::Motion(Motion::Rapid)
        } else if code == self.linear {
            GKind::Motion(Motion::Linear)
        } else if code == self.arc_cw {
            GKind::Motion(Motion::ArcCw)
        } else if code == self.arc_ccw {
            GKind::Motion(Motion::ArcCcw)
        } else if code == self.plane_xy {
            GKind::Plane(Plane::Xy)
        } else if code == self.plane_zx {
            GKind::Plane(Plane::Zx)
        } else if code == self.plane_yz {
            GKind::Plane(Plane::Yz)
        } else if code == self.inch {
            GKind::Inch
        } else if code == self.mm {
            GKind::Mm
        } else if code == self.absolute {
            GKind::Absolute
        } else if code == self.incremental {
            GKind::Incremental
        } else if code == self.arc_centre_absolute {
            GKind::ArcAbsolute
        } else if code == self.arc_centre_incremental {
            GKind::ArcIncremental
        } else if self.work_offsets.contains(&code) {
            GKind::Offset
        } else if code == self.machine_coords {
            GKind::Machine
        } else if code == self.dwell {
            GKind::Dwell
        } else if code == self.cutter_comp_off {
            GKind::CutterOff
        } else if code == self.cutter_comp_left || code == self.cutter_comp_right {
            GKind::CutterComp
        } else if code == self.tool_length_on {
            GKind::ToolLengthOn
        } else if code == self.tool_length_off {
            GKind::ToolLengthOff
        } else if self.home.contains(&code) {
            GKind::Home
        } else if code == self.cancel_cycle {
            GKind::CancelCycle
        } else if (730..=890).contains(&code) {
            // G73 to G89: the canned cycles (G80, the cancel, is matched above).
            GKind::Canned
        } else if let Some(&(_, kind, on)) = self.frame.iter().find(|(c, _, _)| *c == code) {
            GKind::Frame(kind, on)
        } else if self.cycle_mode.contains(&code) || self.ignored.contains(&code) {
            GKind::Ignored
        } else if code == self.set_offsets {
            GKind::SetOffsets
        } else {
            GKind::Unknown
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Word {
    letter: u8,
    value: f64,
    col: u32,
}

/// Machine state. Each axis is known or unknown on its own.
#[derive(Clone, Copy, Debug)]
struct State {
    pos: [Option<f64>; 3],
    motion: Motion,
    plane: Plane,
    /// 1 for millimetres, 25.4 for inches.
    scale: f64,
    absolute: bool,
    arc_absolute: bool,
    feed: f64,
    rpm: f64,
    spindle_on: bool,
    tool: u32,
    pending_tool: u32,
    /// After an unsupported canned cycle, axis words are errors until G80.
    cycle_unsupported: bool,
    /// The frame transforms on, one bit per [`FrameKind`]. While any is on, moves are errors.
    frame_on: u8,
}

/// All three values, or `None` when one axis is unknown.
fn all_known(p: [Option<f64>; 3]) -> Option<[f64; 3]> {
    Some([p[0]?, p[1]?, p[2]?])
}

/// Streams a program line by line. Call [`Parser::push_line`] for each line, then [`Parser::finish`].
pub struct Parser<'a> {
    codes: &'a CodeTable,
    state: State,
    moves: Vec<Move>,
    diags: Vec<Diag>,
    line: u32,
    diag_full: bool,
    moves_full: bool,
    ended: bool,
    warned_after_end: bool,
    warned_tool_length: bool,
    warned_machine: bool,
}

/// Parses a whole program. Input over 64 MB gives one error and no moves.
pub fn parse(text: &str, codes: &CodeTable) -> Program {
    if text.len() > MAX_INPUT_BYTES {
        return Program { tools: Vec::new(), moves: Vec::new(), diags: vec![Diag::error(0, 0, "input is over 64 MB")] };
    }
    let mut parser = Parser::new(codes);
    for line in text.split('\n') {
        parser.push_line(line);
    }
    parser.finish()
}

impl<'a> Parser<'a> {
    pub fn new(codes: &'a CodeTable) -> Self {
        Parser {
            codes,
            state: State {
                pos: [Some(0.0); 3],
                motion: Motion::Rapid,
                plane: Plane::Xy,
                scale: 1.0,
                absolute: true,
                arc_absolute: false,
                feed: 0.0,
                rpm: 0.0,
                spindle_on: false,
                tool: NO_TOOL,
                pending_tool: NO_TOOL,
                cycle_unsupported: false,
                frame_on: 0,
            },
            moves: Vec::new(),
            diags: Vec::new(),
            line: 0,
            diag_full: false,
            moves_full: false,
            ended: false,
            warned_after_end: false,
            warned_tool_length: false,
            warned_machine: false,
        }
    }

    /// The program so far. Tools are left empty: the caller supplies the tool table.
    pub fn finish(self) -> Program {
        Program { tools: Vec::new(), moves: self.moves, diags: self.diags }
    }

    pub fn push_line(&mut self, line: &str) {
        self.line = self.line.saturating_add(1);
        if self.ended {
            let rest = line.trim();
            if !rest.is_empty() && !rest.starts_with('%') && !self.warned_after_end {
                self.warned_after_end = true;
                self.warn(1, "text after M2 or M30 is ignored");
            }
            return;
        }
        if line.len() > MAX_LINE_BYTES {
            self.err(1, "line is over 4 KB");
            return;
        }
        if let Some(words) = self.tokenize(line) {
            self.block(&words);
        }
    }

    fn err(&mut self, col: u32, msg: impl Into<String>) {
        self.push_diag(Diag::error(self.line, col, msg));
    }

    fn warn(&mut self, col: u32, msg: impl Into<String>) {
        self.push_diag(Diag::warning(self.line, col, msg));
    }

    fn push_diag(&mut self, d: Diag) {
        if self.diags.len() < MAX_DIAGS {
            self.diags.push(d);
        } else if !self.diag_full {
            self.diag_full = true;
            self.diags.push(Diag::error(self.line, 0, "too many problems; the rest are not listed"));
        }
    }

    /// Splits a line into words. `None` when the line is skipped (block delete) or has an error.
    fn tokenize(&mut self, line: &str) -> Option<Vec<Word>> {
        let b = line.as_bytes();
        let mut words: Vec<Word> = Vec::new();
        let mut i = 0usize;
        let mut started = false;
        while let Some(&c) = b.get(i) {
            let col = u32::try_from(i + 1).unwrap_or(u32::MAX);
            match c {
                b' ' | b'\t' | b'\r' | b'%' => i += 1,
                b';' => break,
                b'(' => match b.get(i..).and_then(|rest| rest.iter().position(|&x| x == b')')) {
                    Some(p) => i += p + 1,
                    None => {
                        self.err(col, "unterminated comment");
                        return None;
                    }
                },
                b'/' if !started => {
                    if self.codes.block_delete {
                        return None;
                    }
                    i += 1;
                }
                b'#' | b'[' | b']' | b'=' | b'<' | b'>' => {
                    self.err(col, "variables and expressions are not supported");
                    return None;
                }
                c if c.is_ascii_alphabetic() => {
                    if b.get(i + 1).is_some_and(|x| x.is_ascii_alphabetic()) {
                        let mut j = i;
                        while b.get(j).is_some_and(|x| x.is_ascii_alphabetic()) {
                            j += 1;
                        }
                        let word = b.get(i..j).map(|s| String::from_utf8_lossy(s).into_owned()).unwrap_or_default();
                        self.err(col, format!("{word} is not supported"));
                        return None;
                    }
                    let letter = c.to_ascii_uppercase();
                    let (value, end) = match number(b, i + 1) {
                        Ok(v) => v,
                        Err(msg) => {
                            self.err(col, format!("word {} {msg}", letter as char));
                            return None;
                        }
                    };
                    if words.len() >= MAX_WORDS {
                        self.err(col, "more than 64 words on a line");
                        return None;
                    }
                    words.push(Word { letter, value, col });
                    i = end;
                    started = true;
                }
                _ => {
                    self.err(col, "unexpected character");
                    return None;
                }
            }
        }
        Some(words)
    }

    fn code_of(&mut self, w: &Word) -> Option<u32> {
        let t = (w.value * 10.0).round();
        if (w.value * 10.0 - t).abs() > 1e-6 || t < 0.0 || t > 1e6 {
            self.err(w.col, format!("G{} is not a code", w.value));
            return None;
        }
        Some(t as u32)
    }

    fn int_of(&mut self, w: &Word) -> Option<u32> {
        let r = w.value.round();
        if (w.value - r).abs() > 1e-9 || !(0.0..=4.0e9).contains(&r) {
            self.err(w.col, format!("{} {} is not a whole number", w.letter as char, w.value));
            return None;
        }
        Some(r as u32)
    }

    fn block(&mut self, words: &[Word]) {
        let mut gs: Vec<(u32, u32)> = Vec::new();
        let mut ms: Vec<(u32, u32)> = Vec::new();
        let mut tool: Option<u32> = None;
        let mut feed: Option<(f64, u32)> = None;
        let mut spindle: Option<f64> = None;
        let mut axes: [Option<f64>; 3] = [None; 3];
        let mut ijk: [Option<f64>; 3] = [None; 3];
        let mut radius: Option<f64> = None;
        let mut turns: Option<(f64, u32)> = None;
        for w in words {
            match w.letter {
                b'G' => match self.code_of(w) {
                    Some(c) => gs.push((c, w.col)),
                    None => return,
                },
                b'M' => match self.int_of(w) {
                    Some(c) => ms.push((c, w.col)),
                    None => return,
                },
                b'T' => match self.int_of(w) {
                    Some(c) => tool = Some(c),
                    None => return,
                },
                b'X' => axes[0] = Some(w.value),
                b'Y' => axes[1] = Some(w.value),
                b'Z' => axes[2] = Some(w.value),
                b'I' => ijk[0] = Some(w.value),
                b'J' => ijk[1] = Some(w.value),
                b'K' => ijk[2] = Some(w.value),
                b'R' => radius = Some(w.value),
                b'P' => turns = Some((w.value, w.col)),
                b'F' => {
                    if w.value < 0.0 {
                        self.err(w.col, "feed rate is negative");
                        return;
                    }
                    feed = Some((w.value, w.col));
                }
                b'S' => {
                    if w.value < 0.0 {
                        self.err(w.col, "spindle speed is negative");
                        return;
                    }
                    spindle = Some(w.value);
                }
                // Line and program numbers and the tool-length word: read where they are used.
                b'N' | b'O' | b'H' => {}
                b'A' | b'B' | b'C' => {
                    self.err(w.col, "rotary axes are not on the generic 3-axis machine");
                    return;
                }
                other => {
                    self.err(w.col, format!("word {} is not supported", other as char));
                    return;
                }
            }
        }

        // Mode codes first, so the rest of the block reads in the modes it sets.
        let mut block_motion: Option<Motion> = None;
        let mut motion_col = 0u32;
        let mut skip_motion = false;
        let mut home_seen = false;
        let has_axes = axes.iter().any(|a| a.is_some());
        let has_arc_words = ijk.iter().any(|a| a.is_some()) || radius.is_some();
        for (c, col) in gs {
            match self.codes.classify(c) {
                GKind::Motion(m) => {
                    if block_motion.replace(m).is_some() {
                        self.err(col, "two motion codes in one block");
                        return;
                    }
                    motion_col = col;
                    self.state.motion = m;
                }
                GKind::Plane(p) => self.state.plane = p,
                GKind::Inch => self.state.scale = 25.4,
                GKind::Mm => self.state.scale = 1.0,
                GKind::Absolute => self.state.absolute = true,
                GKind::Incremental => self.state.absolute = false,
                GKind::ArcAbsolute => self.state.arc_absolute = true,
                GKind::ArcIncremental => self.state.arc_absolute = false,
                GKind::Offset | GKind::ToolLengthOff | GKind::Ignored => {}
                GKind::Dwell => skip_motion = true,
                GKind::CancelCycle => self.state.cycle_unsupported = false,
                GKind::CutterOff => {}
                GKind::Machine => {
                    // Machine coordinates are not known to the program: the named axes are unknown
                    // after G53, and the block moves nothing.
                    self.forget_axes(axes);
                    skip_motion = true;
                    if !self.warned_machine {
                        self.warned_machine = true;
                        self.warn(col, "G53 moves in machine coordinates: its axes are unknown and nothing is cut");
                    }
                }
                GKind::CutterComp => self.warn(col, "cutter compensation is not applied: the path is shown uncompensated"),
                GKind::ToolLengthOn => {
                    if !self.warned_tool_length {
                        self.warned_tool_length = true;
                        self.warn(col, "tool length offsets are not applied yet");
                    }
                }
                GKind::Home => {
                    self.warn(col, "G28 and G30 are skipped: the home position is unknown, the next move starts a new path");
                    skip_motion = true;
                    home_seen = true;
                }
                GKind::Canned => {
                    // The named axes are unknown now, whatever the cycle would have done.
                    self.forget_axes(axes);
                    self.err(col, "canned cycles are not supported yet; axis words are errors until G80");
                    self.state.cycle_unsupported = true;
                    return;
                }
                GKind::SetOffsets => {
                    self.forget_all();
                    self.err(col, "G10 is not supported yet; the positions after it are unknown");
                    return;
                }
                GKind::Frame(kind, on) => {
                    if !on {
                        // A cancel moves nothing and keeps the positions: they are unknown already.
                        self.state.frame_on &= !kind.bit();
                        continue;
                    }
                    // Frame changes shift every later coordinate: the positions after them are unknown.
                    self.forget_all();
                    self.state.frame_on |= kind.bit();
                    self.err(col, format!("G{} changes the coordinate frame: not supported yet; positions are unknown", c as f64 / 10.0));
                    return;
                }
                GKind::Unknown => {
                    let code = c as f64 / 10.0;
                    // A code this table does not know, with axis or arc words, would move the tool.
                    if has_axes || has_arc_words {
                        self.err(col, format!("G{code} is not supported and would move the tool; the block is skipped"));
                        return;
                    }
                    self.warn(col, format!("G{code} is not supported and is ignored"));
                }
            }
        }
        if home_seen {
            // G28 and G30 name the axes that go home; with none named, every axis is unknown.
            let named = axes.iter().any(|a| a.is_some());
            for (k, a) in axes.iter().enumerate() {
                if let Some(slot) = self.state.pos.get_mut(k).filter(|_| !named || a.is_some()) {
                    *slot = None;
                }
            }
        }

        if let Some(t) = tool {
            self.state.pending_tool = t;
        }
        for (m, col) in ms {
            match m {
                0 | 1 | 7 | 8 | 9 => {}
                2 | 30 => self.ended = true,
                3 | 4 => self.state.spindle_on = true,
                5 => self.state.spindle_on = false,
                6 => self.state.tool = self.state.pending_tool,
                98 | 99 => {
                    self.err(col, "subprograms (M98, M99) are not supported");
                    return;
                }
                _ => self.warn(col, format!("M{m} is ignored")),
            }
        }
        if let Some((f, col)) = feed {
            if f > MAX_VALUE {
                self.err(col, "feed rate is out of range");
                return;
            }
            self.state.feed = f * self.state.scale;
        }
        if let Some(s) = spindle {
            if s > MAX_VALUE {
                self.err(0, "spindle speed is out of range");
                return;
            }
            self.state.rpm = s;
        }
        if skip_motion {
            return;
        }
        if self.state.cycle_unsupported && (has_axes || has_arc_words) {
            // A hole line in cycle mode: its axes are unknown now, whatever the cycle would have done.
            self.forget_axes(axes);
            self.err(motion_col, "axis words are ignored after an unsupported canned cycle; G80 clears it");
            return;
        }
        if self.state.frame_on != 0 && (has_axes || has_arc_words) {
            // The program's coordinates are shifted by a transform that is not applied: no move.
            self.err(motion_col, "a coordinate frame change is active: this move is not simulated until it is cancelled");
            return;
        }
        match self.state.motion {
            Motion::Rapid | Motion::Linear => {
                if !has_axes {
                    return;
                }
                let Some(tip) = self.target(axes, motion_col) else { return };
                let kind = if self.state.motion == Motion::Rapid { MoveKind::Rapid } else { MoveKind::Feed };
                self.push_kind(kind, tip);
            }
            m @ (Motion::ArcCw | Motion::ArcCcw) => {
                // A block with no axis, centre or radius word moves nothing, as G0 and G1 do.
                if !has_axes && !has_arc_words {
                    return;
                }
                // LinuxCNC's P is the total number of turns (at least one); the extra turns are P - 1.
                let extra = match turns {
                    None => 0,
                    Some((p, col)) => {
                        let Some(total) = self.int_of(&Word { letter: b'P', value: p, col }) else { return };
                        if total < 1 {
                            self.err(col, "P is the number of turns and must be at least 1");
                            return;
                        }
                        let extra = total - 1;
                        if extra > MAX_ARC_TURNS {
                            self.err(col, format!("arc turns over {} are not allowed", MAX_ARC_TURNS + 1));
                            return;
                        }
                        extra
                    }
                };
                self.arc(m, axes, ijk, radius, extra, motion_col);
            }
        }
    }

    /// Marks the named axes as unknown (used when the program's motion can no longer be followed).
    /// Every axis is unknown: a frame change or a G10 means the program no longer knows where it is.
    fn forget_all(&mut self) {
        self.state.pos = [None; 3];
    }

    fn forget_axes(&mut self, axes: [Option<f64>; 3]) {
        for (k, a) in axes.iter().enumerate() {
            if let Some(slot) = self.state.pos.get_mut(k).filter(|_| a.is_some()) {
                *slot = None;
            }
        }
    }

    /// The end point of a linear or arc move, per axis: a missing axis keeps its position, which
    /// may be unknown. An incremental move on an unknown axis is an error.
    fn target(&mut self, axes: [Option<f64>; 3], col: u32) -> Option<[Option<f64>; 3]> {
        let absolute = self.state.absolute;
        let mut tip = self.state.pos;
        for (k, a) in axes.iter().enumerate() {
            let Some(v) = *a else { continue };
            let v = v * self.state.scale;
            let base = tip.get(k).copied().flatten();
            let c = if absolute {
                v
            } else {
                match base {
                    Some(b) => b + v,
                    None => {
                        self.err(col, "incremental move on an axis whose position is unknown after G28 or G30");
                        return None;
                    }
                }
            };
            if !c.is_finite() || c.abs() > MAX_POSITION {
                self.err(col, "coordinate is out of range");
                return None;
            }
            if let Some(slot) = tip.get_mut(k) {
                *slot = Some(c);
            }
        }
        Some(tip)
    }

    fn arc(&mut self, m: Motion, axes: [Option<f64>; 3], ijk: [Option<f64>; 3], radius: Option<f64>, turns: u32, col: u32) {
        let (u, v, n) = match self.state.plane {
            Plane::Xy => (0usize, 1usize, 2usize),
            Plane::Zx => (2, 0, 1),
            Plane::Yz => (1, 2, 0),
        };
        let Some(start) = all_known(self.state.pos) else {
            self.err(col, "arc after G28 or G30: the start point is unknown");
            return;
        };
        let ccw = m == Motion::ArcCcw;
        let Some(end_partial) = self.target(axes, col) else { return };
        let Some(end) = all_known(end_partial) else {
            self.err(col, "arc after G28 or G30: give all three axes at its end");
            return;
        };
        let at = |p: [f64; 3], a: usize| p.get(a).copied().unwrap_or(0.0);
        let centre_uv: (f64, f64);
        if let Some(r) = radius {
            if ijk.iter().any(|x| x.is_some()) {
                self.err(col, "arc has both R and I, J, K");
                return;
            }
            let r = r * self.state.scale;
            if r.abs() < 1e-9 {
                self.err(col, "arc radius is zero");
                return;
            }
            let (su, sv) = (at(start, u), at(start, v));
            let (eu, ev) = (at(end, u), at(end, v));
            let (wu, wv) = (eu - su, ev - sv);
            let chord = (wu * wu + wv * wv).sqrt();
            if chord < 1e-9 {
                self.err(col, "R form cannot draw a full circle");
                return;
            }
            // The same tolerance as the centre form, so a rounded semicircle is accepted.
            if chord > 2.0 * r.abs() + 0.002f64.max(0.001 * r.abs()) {
                self.err(col, "arc radius is too small for its end points");
                return;
            }
            let h = (r * r - chord * chord / 4.0).max(0.0).sqrt();
            let side = if ccw == (r > 0.0) { 1.0 } else { -1.0 };
            let (pu, pv) = (-wv / chord, wu / chord);
            let (mu, mv) = ((su + eu) / 2.0, (sv + ev) / 2.0);
            centre_uv = (mu + side * h * pu, mv + side * h * pv);
        } else {
            if ijk.iter().all(|x| x.is_none()) {
                self.err(col, "arc has no centre (I, J or K)");
                return;
            }
            let offset = |a: usize, base: f64| match ijk.get(a).copied().flatten() {
                Some(o) if self.state.arc_absolute => o * self.state.scale,
                Some(o) => base + o * self.state.scale,
                None => base,
            };
            let cu = offset(u, at(start, u));
            let cv = offset(v, at(start, v));
            centre_uv = (cu, cv);
            let rs = ((at(start, u) - cu).powi(2) + (at(start, v) - cv).powi(2)).sqrt();
            let re = ((at(end, u) - cu).powi(2) + (at(end, v) - cv).powi(2)).sqrt();
            if rs < 1e-9 {
                self.err(col, "arc radius is zero");
                return;
            }
            if (rs - re).abs() > 0.002f64.max(0.001 * rs) {
                self.err(col, format!("arc end is {:.4} mm off its circle", (rs - re).abs()));
                return;
            }
        }
        let mut centre = start;
        if let Some(c) = centre.get_mut(u) {
            *c = centre_uv.0;
        }
        if let Some(c) = centre.get_mut(v) {
            *c = centre_uv.1;
        }
        let mut normal = [0.0; 3];
        if let Some(x) = normal.get_mut(n) {
            *x = 1.0;
        }
        self.push_kind(MoveKind::Arc { centre, normal, ccw, turns }, end_partial);
    }

    /// Records a move to `tip`. While any axis is unknown there is no move yet; the first full
    /// tip after an unknown position is a jump.
    fn push_kind(&mut self, kind: MoveKind, tip: [Option<f64>; 3]) {
        let was_known = all_known(self.state.pos).is_some();
        self.state.pos = tip;
        let Some(full) = all_known(tip) else { return };
        if self.moves.len() >= MAX_MOVES {
            if !self.moves_full {
                self.moves_full = true;
                self.err(0, "more than 2,000,000 moves; the rest are dropped");
            }
            return;
        }
        if !was_known && matches!(kind, MoveKind::Feed | MoveKind::Rapid) {
            // A cut from a start the program does not know: it is not simulated, and the program says so.
            self.warn(0, "the start of this cut is unknown (after G28 or G30): the cut from there is not simulated");
        }
        let kind = if was_known { kind } else { MoveKind::Jump };
        let feeding = matches!(kind, MoveKind::Feed | MoveKind::Arc { .. });
        self.moves.push(Move {
            kind,
            tip: full,
            axis: [0.0, 0.0, 1.0],
            joints: None,
            interp: Interp::Tcp,
            tool: self.state.tool,
            feed: if feeding { self.state.feed } else { 0.0 },
            spindle: if self.state.spindle_on { self.state.rpm } else { 0.0 },
            source: self.line,
        });
    }
}

/// A number after a word letter: optional sign, digits, optional fraction. Returns the value and
/// the index after it. Rejects anything that is not finite or is out of range.
fn number(b: &[u8], start: usize) -> Result<(f64, usize), &'static str> {
    let mut j = start;
    if matches!(b.get(j), Some(b'+') | Some(b'-')) {
        j += 1;
    }
    let mut digits = 0usize;
    while b.get(j).is_some_and(|x| x.is_ascii_digit()) {
        j += 1;
        digits += 1;
    }
    if b.get(j) == Some(&b'.') {
        j += 1;
        while b.get(j).is_some_and(|x| x.is_ascii_digit()) {
            j += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return Err("has no value");
    }
    if j - start > MAX_NUMBER_CHARS {
        return Err("has a number that is too long");
    }
    let text = b.get(start..j).and_then(|s| std::str::from_utf8(s).ok()).ok_or("has no value")?;
    let value: f64 = text.parse().map_err(|_| "has a bad number")?;
    if !value.is_finite() || value.abs() > MAX_VALUE {
        return Err("is out of range");
    }
    Ok((value, j))
}

#[cfg(test)]
#[path = "gcode_tests.rs"]
mod tests;
