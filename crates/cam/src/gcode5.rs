//! 4th-axis programs as G-code: the linear axes and a rotary `A` axis, with inverse-time feeds
//! for cuts that turn it.
//!
//! [`emit5`] writes a [`Program5`] (and [`emit_program`] a [`MachineProgram`], which may also
//! hold indexed operations) for one [`Machine`] through a [`Post5`]: the same header, tool
//! changes, spindle, coolant, comments and line numbers as [`crate::emit`], then the motion as
//! `G0`/`G1` blocks with `X Y Z` and `A`, a word written only when it changes. The machine is a
//! 3-axis machine with at most one rotary, lettered `A`; output for a second rotary or for tool
//! centre point control is not written yet. Angles are written in degrees, unwound: a
//! continuous axis adds up past ±360° (a wrapped helix keeps turning one way), as a rotary that
//! does not roll over reads them.
//!
//! `X Y Z` are the machine's linear axes as [`crate::kinematics`] places them, every cut broken
//! into pieces the machine can move through evenly within the post's tolerance. A cut that turns
//! the rotary is timed in inverse time (`G93`, `F` = 1 / the piece's minutes) when the post says
//! so, and `G94` comes back for cuts that do not; otherwise its `F` is the rate of the linear
//! axes (of the rotary alone, in degrees a minute, when it is all that moves) that takes the
//! planned time.
//!
//! An indexed operation turns the rotary once (`G0 A…`) and is then written with the 3-axis
//! rules, arcs kept as `G2`/`G3`.
//!
//! A rapid that turns the rotary first writes the post's `safe_rotate` lines and, with
//! `clearance_retract`, rises to the clearance height (never down to it from higher); then the
//! rotary turns alone, and the linear axes follow, never diagonally: up first when rising,
//! across first when descending. Where the tool's height is not known (the program's start,
//! after a tool change) it first rises to the program's highest clearance. With the same tool,
//! an operation starts from where the last one left the tool. Inverse time (`G93`) never
//! stays in force over a rapid or the blocks between operations (tool change, spindle,
//! coolant), and every `F` is checked to be written as a feed the controller can run (never
//! `F0`). Where every turn is made clear of the work (`safe_rotate` or `clearance_retract`), a
//! run of rapids that each turn the rotary (a link at clearance) is written as its last one:
//! one retract and one turn.
//!
//! The base post's layout switches ([`Post`]: tool list, comments, blank lines, per-operation
//! lines, the safe retract to the machine's top, the tool length offset on the first Z, modal
//! motion codes) are written the same way as by [`crate::emit`]; after a safe retract or a turn
//! of the rotary the next move states its motion code again. With [`Post5::safe_rotate_first`]
//! an operation that starts by turning the rotary takes the `safe_rotate` lines before its
//! comment, and with [`Post5::coolant_after_turn`] its coolant comes on after that first turn
//! rather than before it. A rotary with limits that has room for a whole turn can take a
//! continuous cut past them with [`Rewind`]s.
//!
//! [`parse5`] reads the motion back (the subset these write), which is how the output is
//! checked: see [`crate::readback5`].

use std::f64::consts::{FRAC_PI_4, PI, TAU};

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::gcode::{
    MAX_FEED, MAX_LINES, MAX_OPERATIONS, MIN_FEED, feed_in_range, feed_word_ok, join, number, operation_comment, plain, post_number, tool_list_line,
    words, work_offset_code,
};
use crate::indexed::{IndexedOperation, frame_for_index, head_upright};
use crate::kinematics::{AxisLetter, Carrier, JointMove, Joints, Machine, check_limits, forward, nearest_turn, solve_from};
use crate::multiaxis::{Operation5, Program5, verify5};
use crate::path::{ArcPlane, MAX_MOVES, Move, PlaneArc, arc_sweep};
use crate::post::{CommentStyle, Post, Values, check_template, fill};
use crate::tool::Tool;

use crate::{CamError, Result, invalid};

/// Shortest time an inverse-time block is given (minutes), so its `F` stays finite.
const MIN_MINUTES: f64 = 1e-9;
/// Most decimals of an inverse-time `F`.
const MAX_INVERSE_DECIMALS: u8 = 12;
/// A post processor for 4th-axis programs: a [`Post`] for everything but the motion, and how
/// the rotary is written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Post5 {
    pub base: Post,
    /// Whether cuts that turn the rotary are written in inverse time (`G93`).
    #[serde(default)]
    pub inverse_time: bool,
    /// Decimals of angles (degrees, at most 6).
    pub angle_decimals: u8,
    /// Written before a rapid that turns a rotary, to take the tool clear first (`G53 G0 Z0`:
    /// up to the machine's top in machine coordinates). After it the next rapid states every
    /// linear axis, across first and down last.
    #[serde(default)]
    pub safe_rotate: Vec<String>,
    /// Whether an operation that starts by turning the rotaries takes the `safe_rotate` lines
    /// before its comment and tool change, rather than right before the turn.
    #[serde(default, skip_serializing_if = "is_false")]
    pub safe_rotate_first: bool,
    /// Whether an operation that starts by turning the rotaries switches its coolant on right
    /// after that turn (`G0 A…` then `M8`) rather than before it, after the operation's start
    /// lines. Coolant that goes off, and an operation that turns nothing at its start, are
    /// written as without it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub coolant_after_turn: bool,
    /// Whether a rapid that turns a rotary first rises to the clearance height (`G0 Z…` in the
    /// program's coordinates, after `safe_rotate`), so the part never turns into the tool. The
    /// height is `clearance`, or else the highest rapid of the operation, which is where its
    /// toolpaths retract to; a tool already higher stays where it is. With neither, the
    /// rotaries turn wherever the tool is, which is safe only where the toolpath's rapids
    /// already rise clear (except where its height is not known: then it always rises to the
    /// program's highest clearance first).
    #[serde(default)]
    pub clearance_retract: bool,
    /// The clearance height for `clearance_retract` (mm, program coordinates: on a 4th axis
    /// with the work origin on its centreline, above the largest radius of the stock).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<f64>,
    /// Largest distance of the tip from its planned line (mm).
    pub tolerance: f64,
    /// Largest angle of the tool axis from its planned turn (radians).
    pub angle_tolerance: f64,
    /// Decimals of every inverse-time `F` (at most 12); `None` for as many
    /// as keep about five significant digits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inverse_time_decimals: Option<u8>,
    /// How a continuous cut carries on past the limits of a rotary that has room for a whole
    /// turn; `None` to refuse such a cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewind: Option<Rewind>,
}

/// Rewinds: a continuous cut that would turn a table rotary past its limits (a long wrapped
/// helix on an axis that cannot turn without end) stops where it reaches the limit, draws the
/// tool straight up out of the cut to the operation's clearance (the first [`REWIND_EASE`] at
/// `retract_feed`, the rest at rapid, the tool then clear of the cut it made, as high as the
/// operation's own links turn the rotaries and no higher), takes the post's `safe_rotate`
/// lines (and the rise to the clearance height, with `clearance_retract`), turns the axis back
/// by whole turns (the same position of the part, as far from the limit as the limits allow),
/// comes back at rapid to [`REWIND_EASE`] above the cut (down the room the tool left as it came
/// out) and feeds the rest of the way in at the operation's plunge feed, then goes on. The draw
/// out is shortened to the machine's Z travel, never below that clearance. Only for
/// machine-coordinate output on machines whose rotaries all turn the table; elsewhere the
/// limits stay as they are.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rewind {
    /// How far the tool is drawn out of the cut before the turn when the operation has no
    /// clearance of its own (no rapid to take it from); otherwise it is drawn out to that
    /// clearance (mm).
    pub distance: f64,
    /// Feed of the start of the draw out (mm/min).
    pub retract_feed: f64,
    /// Feed of the end of the way back into the cut for an operation without a plunge feed of
    /// its own ([`Operation5::plunge_feed`], else its tool's) (mm/min).
    pub plunge_feed: f64,
}

/// How far a rewind feeds the tool out of the cut before it rises at rapid, and how far above
/// the cut it comes back at rapid before it feeds the rest of the way in (mm; at most the
/// rewind's distance).
pub const REWIND_EASE: f64 = 1.0;

impl Rewind {
    fn validate(&self) -> Result<()> {
        if !(self.distance > 0.0) || !(self.distance <= 1000.0) {
            return invalid("the rewind distance must be above 0 mm and at most 1000 mm");
        }
        if !feed_in_range(self.retract_feed) || !feed_in_range(self.plunge_feed) {
            return invalid(format!("the rewind feeds must be {MIN_FEED} to {MAX_FEED} mm/min"));
        }
        Ok(())
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn lines(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

impl Post5 {
    /// The 4th-axis setup shared by the built-in posts: inverse time for cuts that turn the
    /// rotary (many controllers otherwise read `F` for a rotary as if degrees were
    /// millimetres), and a rise to the clearance height before a rapid turns it.
    fn four_axis(base: Post) -> Post5 {
        Post5 {
            base,
            inverse_time: true,
            angle_decimals: 4,
            safe_rotate: Vec::new(),
            safe_rotate_first: false,
            coolant_after_turn: false,
            clearance_retract: true,
            clearance: None,
            tolerance: 0.005,
            angle_tolerance: 0.1f64.to_radians(),
            inverse_time_decimals: None,
            rewind: None,
        }
    }

    /// LinuxCNC with a 4th axis: [`Post::rs274`] with incremental arc centres (`G91.1`) set in
    /// the header, and the 4th-axis setup.
    pub fn linuxcnc_4axis() -> Post5 {
        Post5::four_axis(Post {
            name: "linuxcnc-4axis".into(),
            header: lines(&["G17 {units} G40 G49 {work_offset} G80 G90 G91.1 G94"]),
            ..Post::rs274()
        })
    }

    /// grblHAL with a 4th axis: `;` comments, incremental arc centres (`G91.1`) set in the
    /// header, no tool length offset words (a build without a tool changer refuses them), a
    /// manual tool change (spindle off, a comment naming the tool, and a pause, `M0`, for the
    /// operator; the spindle starts again after it), and the 4th-axis setup.
    pub fn grblhal_4axis() -> Post5 {
        Post5::four_axis(Post {
            name: "grblhal-4axis".into(),
            extension: "nc".into(),
            comments: CommentStyle::Semicolon,
            header: lines(&["G90 G91.1 G94 G17 {units}"]),
            tool_change: lines(&["M5", "; change to tool {tool}, diameter {diameter}", "M0"]),
            tool_change_first: false,
            spindle_on: lines(&["M3 S{rpm}"]),
            ..Post::rs274()
        })
    }

    /// The built-in 4th-axis post called `name`.
    pub fn builtin(name: &str) -> Option<Post5> {
        match name.trim().to_ascii_lowercase().as_str() {
            "linuxcnc-4axis" => Some(Post5::linuxcnc_4axis()),
            "grblhal" | "grblhal-4axis" => Some(Post5::grblhal_4axis()),
            _ => None,
        }
    }

    pub const BUILTIN: &'static [&'static str] = &["linuxcnc-4axis", "grblhal-4axis"];

    /// How the toolpaths are turned into machine moves for this post.
    pub fn solve_options(&self) -> crate::kinematics::SolveOptions {
        crate::kinematics::SolveOptions {
            tolerance: self.tolerance,
            angle_tolerance: self.angle_tolerance,
            ..crate::kinematics::SolveOptions::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.base.validate()?;
        if self.angle_decimals > 6 {
            return invalid("a post writes at most 6 decimals for angles");
        }
        check_template(&self.safe_rotate)?;
        if self.clearance.is_some_and(|c| !(c.abs() <= 1.0e6)) {
            return invalid("the clearance height must be a finite number");
        }
        if self.inverse_time_decimals.is_some_and(|d| d > MAX_INVERSE_DECIMALS) {
            return invalid(format!("a post writes at most {MAX_INVERSE_DECIMALS} decimals for inverse-time feeds"));
        }
        if let Some(r) = &self.rewind {
            r.validate()?;
        }
        self.solve_options().validate()
    }
}

/// One operation of a [`MachineProgram`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MachineOperation {
    /// The rotaries move with the cut (or stay where the toolpath keeps the tool axis).
    Continuous(Operation5),
    /// The rotaries turn once and hold still; arcs are kept.
    Indexed(IndexedOperation),
}

/// A program of indexed and continuous multi-axis operations.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineProgram {
    pub name: String,
    pub operations: Vec<MachineOperation>,
    /// Work offset 1 to 6 (`G54` to `G59`); left out, the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_offset: Option<u8>,
}

impl From<&Program5> for MachineProgram {
    fn from(p: &Program5) -> MachineProgram {
        MachineProgram {
            name: p.name.clone(),
            operations: p.operations.iter().cloned().map(MachineOperation::Continuous).collect(),
            work_offset: p.work_offset,
        }
    }
}

/// How a cut's `F` is meant.
#[derive(Clone, Copy)]
enum Rate {
    /// mm/min, written in the post's units.
    Linear(f64),
    /// Degrees a minute.
    Degrees(f64),
    /// 1 / minutes.
    Inverse(f64),
}

/// What the writer knows of the tool's height.
#[derive(Clone, Copy, PartialEq)]
enum Height {
    /// Nothing (the program's start, after a tool change): the next rapid rises before it
    /// moves across, and a turn of the rotary rises to the program's highest clearance first.
    Unknown,
    /// At the machine's top, after the post's `safe_rotate`.
    Top,
    /// At this height (the program's coordinates).
    At(f64),
}

struct Writer5<'a> {
    post: &'a Post,
    angle_decimals: u8,
    /// Fixed decimals of an inverse-time `F` ([`Post5::inverse_time_decimals`]).
    inverse_decimals: Option<u8>,
    out: String,
    line: Option<u32>,
    /// The linear words as written, and the position they say.
    lin: [Option<String>; 3],
    pos: Option<Vec3>,
    /// The tool's height, which may be known while X and Y are not.
    height: Height,
    /// The rotary words as written, in joint order.
    rot: Vec<Option<String>>,
    /// Joint indices and letters, in letter order (the order the words are written in).
    order: Vec<(usize, char)>,
    feed: Option<String>,
    inverse: bool,
    /// The motion mode in force as written (`G0` to `G3`), when known.
    mode: Option<u8>,
    /// The tool whose length offset goes with the next Z word ([`Post::tool_length_on_first_z`]).
    length_offset: Option<u32>,
    /// Whether the post sets the tool length offset with the first Z word.
    length_on_z: bool,
    /// The tool in the spindle.
    tool: Option<u32>,
    /// Lines held back until the operation's first turn of the rotaries is written
    /// ([`Post5::coolant_after_turn`]), or until any other motion if that comes first.
    held: Vec<String>,
    /// The plane arcs are in as written: `G17` (the headers' plane) until a vertical arc.
    plane: ArcPlane,
}

impl Writer5<'_> {
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

    /// The lines of a template. A template may hold any motion code, so the next motion states
    /// its own; one that sets feed per minute (`G94`) ends inverse time.
    fn template(&mut self, lines: &[String], values: &Values) -> Result<()> {
        let filled = lines.iter().map(|line| fill(line, values)).collect::<Result<Vec<String>>>()?;
        self.filled(&filled);
        Ok(())
    }

    /// Template lines already filled in, written as [`Writer5::template`] writes them.
    fn filled(&mut self, lines: &[String]) {
        for text in lines {
            if words(text, 0).is_ok_and(|ws| ws.iter().any(|&(c, v)| c == 'G' && (v * 10.0).round() as i64 == 940)) {
                (self.inverse, self.feed) = (false, None);
            }
            self.block(text);
        }
        if !lines.is_empty() {
            self.mode = None;
        }
    }

    /// Holds a template's lines back until the next turn of the rotaries has been written (or
    /// any other motion comes first).
    fn hold(&mut self, lines: &[String], values: &Values) -> Result<()> {
        for line in lines {
            self.held.push(fill(line, values)?);
        }
        Ok(())
    }

    /// The held lines, if any.
    fn release(&mut self) {
        if !self.held.is_empty() {
            let held = std::mem::take(&mut self.held);
            self.filled(&held);
        }
    }

    /// Safe lines (the post's safe retract or `safe_rotate`) unless the tool is still at the
    /// machine's top from the last: afterwards it is, every linear axis is stated again, and
    /// the next Z move sets the tool length offset again if the post sets it with the first Z.
    fn safe_lines(&mut self, lines: &[String], values: &Values) -> Result<()> {
        if lines.is_empty() || self.height == Height::Top {
            return Ok(());
        }
        // A return home is a rapid: feed per minute again first.
        self.per_minute();
        self.template(lines, values)?;
        (self.lin, self.pos, self.height) = ([None, None, None], None, Height::Top);
        if self.length_on_z {
            self.length_offset = self.tool;
        }
        Ok(())
    }

    fn value(&self, v: f64, decimals: u8) -> String {
        post_number(self.post, v, decimals)
    }

    fn coord(&self, mm: f64) -> String {
        self.value(mm / self.post.units.mm(), self.post.decimals)
    }

    fn angle(&self, radians: f64) -> String {
        self.value(radians.to_degrees(), self.angle_decimals)
    }

    /// The word of motion code `n`; nothing for a modal post already in that mode.
    fn motion(&mut self, n: u8) -> String {
        if self.post.modal_motion && self.mode == Some(n) {
            return String::new();
        }
        self.mode = Some(n);
        if self.post.two_digit_codes { format!("G{n:02}") } else { format!("G{n}") }
    }

    /// Forget where the linear axes are (after a block that may move them, such as a tool
    /// change): the next move states each of them, and the next rapid rises first.
    fn forget(&mut self) {
        (self.lin, self.pos, self.height, self.feed) = ([None, None, None], None, Height::Unknown, None);
    }

    /// `G94` if inverse time is in force, so the blocks that follow (a tool change, the spindle,
    /// a macro the controller runs for them) read any `F` per minute.
    fn per_minute(&mut self) {
        if self.inverse {
            self.block("G94");
            (self.inverse, self.feed) = (false, None);
        }
    }

    /// Whether moving the rotaries to `rot` changes a word.
    fn turns(&self, rot: &[f64]) -> bool {
        self.order.iter().any(|&(i, _)| rot.get(i).is_some_and(|&a| self.rot.get(i).and_then(Option::as_ref) != Some(&self.angle(a))))
    }

    /// The rotary words that change, noted as written.
    fn rotary_words(&mut self, rot: &[f64]) -> Vec<String> {
        let mut words = Vec::new();
        for k in 0..self.order.len() {
            let Some(&(i, c)) = self.order.get(k) else { continue };
            let Some(&a) = rot.get(i) else { continue };
            let s = self.angle(a);
            if let Some(slot) = self.rot.get_mut(i)
                && slot.as_ref() != Some(&s)
            {
                words.push(format!("{c}{s}"));
                *slot = Some(s);
            }
        }
        words
    }

    /// The linear words among `xy` and `z` that change, noted as written; with the tool length
    /// offset (`G43 … H`) around the Z word if it is due.
    fn linear_words(&mut self, to: Vec3, xy: bool, z: bool) -> Vec<String> {
        let new = [self.coord(to.x), self.coord(to.y), self.coord(to.z)];
        let mut words = Vec::new();
        let mut offset = None;
        for (k, (letter, s)) in ['X', 'Y', 'Z'].into_iter().zip(new).enumerate() {
            let wanted = if k < 2 { xy } else { z };
            if let Some(slot) = self.lin.get_mut(k)
                && wanted
                && slot.as_ref() != Some(&s)
            {
                if k == 2 {
                    offset = self.length_offset.take();
                }
                words.push(format!("{letter}{s}"));
                *slot = Some(s);
            }
        }
        if let Some(t) = offset {
            words.insert(0, "G43".into());
            words.push(format!("H{t}"));
        }
        words
    }

    /// A rapid of the linear axes named, if that changes a word; `G94` first if inverse time
    /// is in force, so no rapid is ever written under `G93`.
    fn rapid_block(&mut self, to: Vec3, xy: bool, z: bool) {
        let words = self.linear_words(to, xy, z);
        if !words.is_empty() {
            self.per_minute();
            let mut block = vec![self.motion(0)];
            block.extend(words);
            self.block(&join(&block));
        }
    }

    /// Turns the rotaries to `rot` at rapid if that changes a word: the post's safe lines, the
    /// rise to `clearance`, then the rotaries alone. The rise never goes down: a tool above the
    /// clearance stays where it is, one at the machine's top after the safe lines stays there,
    /// and one whose height is not known rises to `top` (the program's highest clearance) or
    /// `clearance`, whichever is higher, even when the post asks for no rise.
    fn turn(&mut self, rot: &[f64], safe: &[String], clearance: Option<f64>, top: Option<f64>, values: &Values) -> Result<()> {
        if !self.turns(rot) {
            return Ok(());
        }
        self.per_minute();
        self.safe_lines(safe, values)?;
        let rise = match self.height {
            Height::Top => None,
            Height::Unknown => clearance.into_iter().chain(top).reduce(f64::max),
            Height::At(z) => clearance.filter(|&c| c > z),
        };
        if let Some(h) = rise {
            let at = self.pos.map_or(Vec3::new(0.0, 0.0, h), |p| Vec3::new(p.x, p.y, h));
            self.rapid_block(at, false, true);
            self.pos = self.pos.map(|_| at);
            self.height = Height::At(h);
        }
        // The turn states its motion code, alone on its block as it is; the move after it is
        // still a rapid and does not repeat it.
        self.mode = None;
        let mut block = vec![self.motion(0)];
        block.extend(self.rotary_words(rot));
        self.block(&join(&block));
        self.release();
        Ok(())
    }

    /// A rapid of the linear axes, never a diagonal: up first when rising, across first when
    /// descending. Where the tool's height is not known it first rises to `rise` (or the
    /// rapid's own height, if higher), so it never moves across at a height it does not know.
    fn rapid(&mut self, to: Vec3, rise: Option<f64>) {
        self.release();
        let up_first = match self.height {
            Height::At(z) => to.z > z,
            Height::Top => false,
            Height::Unknown => {
                let up = rise.map_or(to.z, |r| r.max(to.z));
                self.rapid_block(Vec3::new(to.x, to.y, up), false, true);
                false
            }
        };
        if up_first {
            self.rapid_block(to, false, true);
            self.rapid_block(to, true, false);
        } else {
            self.rapid_block(to, true, false);
            self.rapid_block(to, false, true);
        }
        (self.pos, self.height) = (Some(to), Height::At(to.z));
    }

    /// ` F…` in feed per minute when it changes.
    fn feed_word(&mut self, f: String) -> String {
        if self.feed.as_ref() == Some(&f) {
            return String::new();
        }
        self.feed = Some(f.clone());
        format!("F{f}")
    }

    /// `G93`/`G94` when the feed mode changes to `inverse`.
    fn feed_mode(&mut self, inverse: bool) -> &'static str {
        if inverse == self.inverse {
            return "";
        }
        self.inverse = inverse;
        self.feed = None;
        if inverse { "G93" } else { "G94" }
    }

    /// The ` F…` of a cut; refused when it would be written as `F0` or as an absurd rate.
    fn rate_word(&mut self, rate: Rate) -> Result<String> {
        let (f, per_minute) = match rate {
            Rate::Linear(mm) => (self.value(mm / self.post.units.mm(), self.post.feed_decimals), true),
            Rate::Degrees(deg) => (self.value(deg, self.post.feed_decimals), true),
            // Inverse time wants its F on every block, with the post's decimals or enough digits
            // to keep about five significant ones, so a long, slow cut is never written as F0.
            Rate::Inverse(v) => {
                let digits = self
                    .inverse_decimals
                    .unwrap_or_else(|| (4.0 - v.log10().floor()).clamp(f64::from(self.post.feed_decimals), f64::from(MAX_INVERSE_DECIMALS)) as u8);
                (self.value(v, digits), false)
            }
        };
        if !feed_word_ok(&f, per_minute) {
            return Err(CamError::Unsafe(format!("a cut would be written with the feed F{f}, which the controller cannot run")));
        }
        Ok(if per_minute { self.feed_word(f) } else { format!("F{f}") })
    }

    fn feed(&mut self, to: Vec3, rot: &[f64], rate: Rate) -> Result<()> {
        self.release();
        let mut words = self.linear_words(to, true, true);
        words.extend(self.rotary_words(rot));
        (self.pos, self.height) = (Some(to), Height::At(to.z));
        if words.is_empty() {
            return Ok(());
        }
        let mode = self.feed_mode(matches!(rate, Rate::Inverse(_))).to_string();
        let f = self.rate_word(rate)?;
        let mut block = vec![mode, self.motion(1)];
        block.extend(words);
        block.push(f);
        self.block(&join(&block));
        Ok(())
    }

    /// An arc in the machine's XY plane, the rotaries held: as [`crate::emit`] writes one.
    fn arc(&mut self, to: Vec3, center: Vec2, ccw: bool, feed: f64, rot: &[f64]) -> Result<()> {
        self.release();
        let Some(from) = self.pos else { return Err(CamError::Unsafe("an arc before the first positioning move".into())) };
        let (nx, ny) = (self.coord(to.x), self.coord(to.y));
        let same_x = self.lin.first().and_then(Option::as_ref) == Some(&nx);
        let same_y = self.lin.get(1).and_then(Option::as_ref) == Some(&ny);
        // An arc whose end is written the same as its start would be read as a full circle.
        if same_x && same_y {
            return self.feed(to, rot, Rate::Linear(feed));
        }
        let nz = self.coord(to.z);
        let helix = self.lin.get(2).and_then(Option::as_ref) != Some(&nz);
        if !self.post.arcs || (helix && !self.post.helical_arcs) {
            let start = Vec2::new(from.x, from.y) - center;
            let (r, sweep) = (start.len(), arc_sweep(from, to, center, ccw));
            let tol = self.post.arc_tolerance;
            let step = if tol < r { 2.0 * (1.0 - tol / r).acos() } else { FRAC_PI_4 };
            let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 4096);
            for i in 1..n {
                let t = i as f64 / n as f64;
                let p = center + Vec2::from_angle(start.angle() + sweep * t) * r;
                self.feed(p.extend(from.z + (to.z - from.z) * t), rot, Rate::Linear(feed))?;
            }
            return self.feed(to, rot, Rate::Linear(feed));
        }
        let mode = self.feed_mode(false).to_string();
        let plane = self.plane_word(ArcPlane::Xy);
        if !plane.is_empty() {
            // After a change of plane the motion code is written again, so the block reads on its own.
            self.mode = None;
        }
        let mut words = vec![mode, plane, self.motion(if ccw { 3 } else { 2 })];
        // A modal post leaves out an end coordinate that does not change.
        if !self.post.modal_motion || !same_x {
            words.push(format!("X{nx}"));
        }
        if !self.post.modal_motion || !same_y {
            words.push(format!("Y{ny}"));
        }
        if helix {
            words.push(format!("Z{nz}"));
        }
        words.push(format!("I{}", self.coord(center.x - from.x)));
        words.push(format!("J{}", self.coord(center.y - from.y)));
        let f = self.rate_word(Rate::Linear(feed))?;
        words.push(f);
        self.lin = [Some(nx), Some(ny), Some(nz)];
        (self.pos, self.height) = (Some(to), Height::At(to.z));
        self.block(&join(&words));
        Ok(())
    }

    /// The plane word (`G17`, `G18`, `G19`) when `plane` is not the one in force, written on the
    /// arc's own block, before its motion code.
    fn plane_word(&mut self, plane: ArcPlane) -> String {
        if self.plane == plane {
            return String::new();
        }
        self.plane = plane;
        format!("G{}", plane.code())
    }

    /// Back to `G17` on a block of its own, after an operation that wrote a vertical arc, so
    /// that what follows (canned cycles, another post's arcs) is in the plane the header set.
    fn plane_xy(&mut self) {
        if self.plane != ArcPlane::Xy {
            self.release();
            let w = self.plane_word(ArcPlane::Xy);
            self.block(&w);
        }
    }

    /// An arc in `plane` (`G18` ZX or `G19` YZ) about `center`, the rotaries held: the plane
    /// word, the end's two coordinates in the plane (a modal post leaves out one that does not
    /// change), the coordinate along the normal when it changes (a helix), and both centre
    /// offsets of the plane (`I J K` for X Y Z). Lines within the post's arc tolerance when the
    /// post writes no arcs (or no helices, for a helix).
    fn plane_arc(&mut self, to: Vec3, center: Vec3, plane: ArcPlane, ccw: bool, feed: f64, rot: &[f64]) -> Result<()> {
        if plane == ArcPlane::Xy {
            return self.arc(to, Vec2::new(center.x, center.y), ccw, feed, rot);
        }
        self.release();
        let Some(from) = self.pos else { return Err(CamError::Unsafe("an arc before the first positioning move".into())) };
        let (u, v, n) = plane.axes();
        let pick = |p: Vec3, i: usize| match i {
            0 => p.x,
            1 => p.y,
            _ => p.z,
        };
        let new: [String; 3] = [self.coord(to.x), self.coord(to.y), self.coord(to.z)];
        let unchanged: Vec<bool> = (0..3).map(|i| self.lin.get(i).and_then(Option::as_ref) == new.get(i)).collect();
        let same = |i: usize| unchanged.get(i).copied().unwrap_or(false);
        // An arc whose end is written the same as its start would be read as a full circle.
        if same(u) && same(v) {
            return self.feed(to, rot, Rate::Linear(feed));
        }
        let helix = !same(n);
        if !self.post.arcs || (helix && !self.post.helical_arcs) {
            let arc = PlaneArc { from, to, center, plane, ccw };
            let points = arc.points(self.post.arc_tolerance);
            for p in points {
                self.feed(p, rot, Rate::Linear(feed))?;
            }
            return Ok(());
        }
        let mode = self.feed_mode(false).to_string();
        let word = self.plane_word(plane);
        if !word.is_empty() {
            self.mode = None;
        }
        let mut words = vec![mode, word, self.motion(if ccw { 3 } else { 2 })];
        for (i, letter) in ['X', 'Y', 'Z'].into_iter().enumerate() {
            let in_plane = i == u || i == v;
            let write = if in_plane { !self.post.modal_motion || !same(i) } else { helix };
            if write && let Some(w) = new.get(i) {
                words.push(format!("{letter}{w}"));
            }
        }
        for (i, letter) in ['I', 'J', 'K'].into_iter().enumerate() {
            if i == u || i == v {
                words.push(format!("{letter}{}", self.coord(pick(center, i) - pick(from, i))));
            }
        }
        let f = self.rate_word(Rate::Linear(feed))?;
        words.push(f);
        self.lin = new.map(Some);
        (self.pos, self.height) = (Some(to), Height::At(to.z));
        self.block(&join(&words));
        Ok(())
    }

    fn dwell(&mut self, seconds: f64, template: &str, values: &mut Values) -> Result<()> {
        if !(seconds >= 0.0) || !(seconds <= 1.0e6) {
            return Err(CamError::Unsafe("a dwell must be 0 seconds or longer".into()));
        }
        self.release();
        values.seconds = self.value(seconds, 3);
        values.milliseconds = number(seconds * 1000.0, 0);
        let dwell = fill(template, values)?;
        self.block(&dwell);
        Ok(())
    }

    /// A rewind at `from` (see [`Rewind`]): out of the cut along the machine's Z (fed clear of
    /// it, then at rapid, within the Z travel), the safe lines and the turn back to `back`,
    /// rapid down to just above the cut, and fed back into it. The minutes at feed it adds.
    fn rewind(&mut self, from: &Joints, back: &[f64], r: &RewindAt<'_>, turn: &Turn<'_>, values: &Values) -> Result<f64> {
        let ease = REWIND_EASE.min(r.rewind.distance);
        let z = from.linear.z;
        // To the operation's clearance, where its own links turn the rotaries (outside the
        // stock, as the operation was planned): never lower, as a post that turns them wherever
        // the tool is would otherwise turn a short draw out with the tool still in the groove,
        // and never higher, as nothing the operation asked for needs more room and a machine
        // with little Z travel above the part could not run it. Only an operation with no
        // clearance of its own is drawn out by the rewind's distance.
        let floor = r.floor.unwrap_or(f64::NEG_INFINITY);
        let wanted = match r.floor {
            Some(c) => c.max(z + ease),
            None => z + r.rewind.distance,
        };
        let mut top = wanted;
        if let Some((lo, hi)) = r.machine.linear_limits {
            // Shortened to the travel left, never below the operation's clearance.
            let need = floor.max(z + ease);
            if hi.z < need - 1e-9 {
                let at: Vec<String> =
                    r.machine.rotaries.iter().zip(&from.rotary).map(|(a, v)| format!("{}{:.3}°", a.letter.char(), v.to_degrees())).collect();
                return Err(CamError::Unsafe(format!(
                    "a rewind at {} needs Z up to {need:.3} mm to draw the tool out of the cut{}, beyond the Z travel {:.3} to {:.3} mm",
                    at.join(" "),
                    if floor > z + ease { " to the operation's clearance" } else { "" },
                    lo.z,
                    hi.z
                )));
            }
            top = wanted.min(hi.z);
        }
        let eased = Vec3::new(from.linear.x, from.linear.y, z + ease);
        let out = Vec3::new(from.linear.x, from.linear.y, top.max(eased.z));
        check_limits(r.machine, &Joints { linear: out, rotary: from.rotary.clone() })?;
        check_limits(r.machine, &Joints { linear: eased, rotary: back.to_vec() })?;
        self.feed(eased, &from.rotary, Rate::Linear(r.rewind.retract_feed))?;
        if out.z > eased.z {
            self.rapid(out, None);
        }
        self.turn(back, turn.safe, turn.clearance, turn.top, values)?;
        // Down the room the tool left as it came out, at the same X and Y.
        self.rapid(eased, turn.top);
        self.feed(from.linear, back, Rate::Linear(r.plunge_feed))?;
        Ok(ease / r.rewind.retract_feed + ease / r.plunge_feed)
    }
}

/// How an operation's rewinds draw out and come back (see [`Writer5::rewind`]).
struct RewindAt<'a> {
    rewind: &'a Rewind,
    /// Feed of the end of the way back into the cut (mm/min).
    plunge_feed: f64,
    /// The operation's clearance height (its highest rapid, or the post's `clearance` where
    /// that is higher): the draw out ends there, and one shortened to the machine's Z travel
    /// never ends below it.
    floor: Option<f64>,
    machine: &'a Machine,
}

/// What the writer tells of one operation as written, beyond its toolpath.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WrittenOperation {
    /// Rewinds of the rotaries in it ([`Rewind`]).
    pub rewinds: usize,
    /// Minutes at feed the rewinds add: the start of each draw out and the end of each way
    /// back in.
    pub rewind_minutes: f64,
}

/// How the rotaries turn at rapid in an operation: the safe lines, the clearance height and the
/// program's highest clearance (see [`Writer5::turn`]).
struct Turn<'a> {
    safe: &'a [String],
    clearance: Option<f64>,
    top: Option<f64>,
}

/// Writes `program` for `machine` through `post`.
pub fn emit5(program: &Program5, post: &Post5, machine: &Machine) -> Result<String> {
    let ops: Vec<Op<'_>> = program.operations.iter().map(Op::Continuous).collect();
    Ok(emit_ops(&program.name, program.work_offset, &ops, post, machine)?.0)
}

/// Writes `program`, its indexed operations with their arcs kept, for `machine` through `post`.
/// Indexed operations need the head upright (a tilted head would need a tilted work plane).
pub fn emit_program(program: &MachineProgram, post: &Post5, machine: &Machine) -> Result<String> {
    Ok(emit_program_report(program, post, machine)?.0)
}

/// [`emit_program`], and what was written of each operation beyond its toolpath (its rewinds),
/// in program order.
pub fn emit_program_report(program: &MachineProgram, post: &Post5, machine: &Machine) -> Result<(String, Vec<WrittenOperation>)> {
    let ops: Vec<Op<'_>> = program
        .operations
        .iter()
        .map(|o| match o {
            MachineOperation::Continuous(c) => Op::Continuous(c),
            MachineOperation::Indexed(i) => Op::Indexed(i),
        })
        .collect();
    emit_ops(&program.name, program.work_offset, &ops, post, machine)
}

#[derive(Clone, Copy)]
enum Op<'a> {
    Continuous(&'a Operation5),
    Indexed(&'a IndexedOperation),
}

/// The motion of one operation, ready to write.
enum Motion {
    /// Machine moves, and the rewinds among them: `(index of the move a rewind comes before,
    /// the angles the rotaries turn back to)`.
    Joints {
        moves: Vec<JointMove>,
        rewinds: Vec<(usize, Vec<f64>)>,
    },
    Indexed {
        rotary: Vec<f64>,
        path: Vec<Move>,
    },
}

fn in_op(name: &str) -> impl Fn(CamError) -> CamError + '_ {
    move |e| match e {
        CamError::Unsafe(s) => CamError::Unsafe(format!("operation `{name}`: {s}")),
        CamError::Invalid(s) => CamError::Invalid(format!("operation `{name}`: {s}")),
        CamError::TooLarge(s) => CamError::TooLarge(format!("operation `{name}`: {s}")),
        other => other,
    }
}

/// An indexed operation's moves checked: finite, feeds [`MIN_FEED`] to [`MAX_FEED`] mm/min,
/// within the machine's travel.
fn check_indexed(op: &IndexedOperation, machine: &Machine) -> Result<()> {
    if op.path.moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
    }
    let mut started = false;
    for (i, m) in op.path.moves.iter().enumerate() {
        if m.target().is_some_and(|to| !to.is_finite() || !(to.len() <= 1.0e7)) {
            return Err(CamError::Unsafe(format!("move {i}: a coordinate is not a finite number")));
        }
        match *m {
            Move::Rapid { .. } => started = true,
            Move::Feed { feed, .. } | Move::Arc { feed, .. } | Move::PlaneArc { feed, .. } => {
                if !started {
                    return Err(CamError::Unsafe(format!("move {i}: a cut before the first positioning move")));
                }
                if !feed_in_range(feed) {
                    return Err(CamError::Unsafe(format!("move {i}: the feed must be {MIN_FEED} to {MAX_FEED} mm/min")));
                }
            }
            Move::Dwell { seconds } => {
                if !(seconds >= 0.0) || !(seconds <= 1.0e6) {
                    return Err(CamError::Unsafe(format!("move {i}: a dwell must be 0 seconds or longer")));
                }
            }
        }
    }
    // The machine is at X = p for every point p of the path in its frame.
    for (p, _) in op.path.points(0.01) {
        check_limits(machine, &Joints { linear: p, rotary: op.rotary.clone() })?;
    }
    Ok(())
}

/// One operation solved and ready to write.
struct Planned<'a> {
    name: &'a str,
    tool: &'a Tool,
    rpm: f64,
    coolant: bool,
    motion: Motion,
    /// The highest rapid (the machine's Z), where the operation's toolpaths retract to.
    top: Option<f64>,
    /// The lowest Z of the operation, in the same coordinates.
    low: Option<f64>,
    /// Whether any rotary is away from zero in it.
    turned: bool,
    /// The feed of its plunges (mm/min), for the way back into the cut after a rewind.
    plunge: Option<f64>,
}

/// Which rapids of `moves` (an operation's machine moves, after `previous`) are left out: in
/// a run of rapids that each turn a rotary (a link at clearance that turns the part a step at a
/// time), all but the last, so the link is one safe retract and one turn rather than a
/// staircase of them. Only where every turn of the rotaries is made clear of the work (the
/// post's `safe_rotate` lines or its rise to the clearance height), and only when the rotaries
/// each keep turning one way through the run, so the one turn sweeps the same angles.
fn link_steps(moves: &[JointMove], previous: Option<&Joints>, post: &Post5) -> Vec<bool> {
    let mut skip = vec![false; moves.len()];
    if post.safe_rotate.is_empty() && !post.clearance_retract {
        return skip;
    }
    let turning = |from: Option<&Joints>, to: &Joints| from.is_none_or(|f| f.rotary_change(to) > 1e-12);
    let way = |d: f64| {
        if d > 1e-12 {
            1.0
        } else if d < -1e-12 {
            -1.0
        } else {
            0.0
        }
    };
    let mut before: Option<&Joints> = previous;
    // The way each rotary turns through the run of turning rapids the last move is in (0: not
    // yet), or `None` outside one.
    let mut run: Option<Vec<f64>> = None;
    for (k, m) in moves.iter().enumerate() {
        let next = match m {
            JointMove::Rapid { to, .. } if turning(before, to) => {
                let dirs: Vec<f64> = match before {
                    Some(b) => b.rotary.iter().zip(&to.rotary).map(|(a, c)| way(c - a)).collect(),
                    None => vec![0.0; to.rotary.len()],
                };
                match run.take() {
                    Some(run_dirs) if run_dirs.iter().zip(&dirs).all(|(r, d)| *r == 0.0 || *d == 0.0 || r == d) => {
                        if let Some(s) = k.checked_sub(1).and_then(|p| skip.get_mut(p)) {
                            *s = true;
                        }
                        Some(run_dirs.iter().zip(&dirs).map(|(r, d)| if *r == 0.0 { *d } else { *r }).collect())
                    }
                    _ => Some(dirs),
                }
            }
            _ => None,
        };
        run = next;
        if let Some(j) = m.joints() {
            before = Some(j);
        }
    }
    skip
}

/// The angles the rotaries turn to at an operation's first rapid.
fn first_turn(motion: &Motion) -> Option<&[f64]> {
    match motion {
        Motion::Indexed { rotary, path } => (!path.is_empty()).then_some(rotary.as_slice()),
        Motion::Joints { moves, .. } => moves.iter().find_map(|m| match m {
            JointMove::Rapid { to, .. } => Some(to.rotary.as_slice()),
            _ => None,
        }),
    }
}

/// Slack (radians) of an angle against a limit.
const LIMIT_SLACK: f64 = 1e-9;
/// How far past a limit (radians, 0.0006°) a cut's angle is taken back to the limit rather than
/// rewound: a path drawn a hair longer than a whole turn (to rounding, below what the angle is
/// written to) ends at the limit, rather than turning A back two turns to cut the hair.
const LIMIT_SNAP: f64 = 1e-5;

/// `j`'s rotaries no more than [`LIMIT_SNAP`] past `machine`'s limits brought back to them.
fn snap_to_limits(machine: &Machine, j: &mut Joints) {
    for (r, a) in machine.rotaries.iter().zip(j.rotary.iter_mut()) {
        if let Some((lo, hi)) = r.limits {
            if *a > hi && *a <= hi + LIMIT_SNAP {
                *a = hi;
            } else if *a < lo && *a >= lo - LIMIT_SNAP {
                *a = lo;
            }
        }
    }
}
/// Most rewinds within one machine move.
const MAX_REWINDS_PER_MOVE: usize = 64;

/// `machine` with the limits left off its table rotaries that have room for a whole turn (every
/// angle has an equal one within them), to solve continuous operations on for a post that
/// rewinds; `None` if there are none, or if a rotary turns the head (the draw out of the cut
/// would then not be along Z).
fn turning_freely(machine: &Machine) -> Option<Machine> {
    if machine.rotaries.iter().any(|r| r.carrier == Carrier::Head) {
        return None;
    }
    let mut free = machine.clone();
    let mut any = false;
    for r in &mut free.rotaries {
        if r.limits.is_some_and(|(lo, hi)| hi - lo >= TAU - LIMIT_SLACK) {
            r.limits = None;
            any = true;
        }
    }
    any.then_some(free)
}

/// The rotaries of `j` beyond `machine`'s limits: `(joint, lowest, highest)`.
fn beyond(machine: &Machine, j: &Joints) -> Vec<(usize, f64, f64)> {
    machine
        .rotaries
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let (lo, hi) = r.limits?;
            let a = *j.rotary.get(i)?;
            (a < lo - LIMIT_SLACK || a > hi + LIMIT_SLACK).then_some((i, lo, hi))
        })
        .collect()
}

/// Machine moves brought within the limits, and the rewinds among them (see [`Motion::Joints`]).
type Unwound = (Vec<JointMove>, Vec<(usize, Vec<f64>)>);

/// Most starting turns [`unwind`] tries for one operation.
const MAX_START_TURNS: usize = 8;

/// The moves of a continuous operation solved on [`turning_freely`]'s machine brought within
/// `machine`'s limits ([`unwind_from`]), starting at the whole turn that needs the fewest
/// rewinds: an operation that would start at a limit and run on past it starts a turn or more
/// away instead, so its range of angles sits inside the limits as far as it can. Of starts with
/// as few rewinds, the one nearest where the rotaries were (the start the operation was solved
/// at, if it is within the limits).
fn unwind(machine: &Machine, moves: &[JointMove], previous: Option<&Joints>) -> Result<Unwound> {
    let n = machine.rotaries.len();
    let Some(first) = moves.iter().find_map(|m| match m {
        JointMove::Rapid { to, .. } => Some(to.rotary.clone()),
        _ => None,
    }) else {
        return unwind_from(machine, moves, previous, vec![0.0; n]);
    };
    // Each rotary's whole-turn shifts that bring the first rapid within its limits, nearest the
    // angle it was solved at first.
    let mut per_axis: Vec<Vec<f64>> = Vec::with_capacity(n);
    for (i, r) in machine.rotaries.iter().enumerate() {
        let a = first.get(i).copied().unwrap_or(0.0);
        let mut shifts = vec![0.0];
        if let Some((lo, hi)) = r.limits
            && a.is_finite()
        {
            let kmin = ((lo - LIMIT_SLACK - a) / TAU).ceil();
            let kmax = ((hi + LIMIT_SLACK - a) / TAU).floor();
            if kmin <= kmax {
                // Outward from the turn nearest the one it was solved at.
                let k0 = 0f64.clamp(kmin, kmax);
                shifts = (0..2 * MAX_START_TURNS)
                    .map(|j| k0 + if j % 2 == 0 { (j / 2) as f64 } else { -(j.div_ceil(2) as f64) })
                    .filter(|k| (kmin..=kmax).contains(k))
                    .take(MAX_START_TURNS)
                    .map(|k| k * TAU)
                    .collect();
            }
        }
        per_axis.push(shifts);
    }
    let mut starts: Vec<Vec<f64>> = vec![Vec::new()];
    for shifts in &per_axis {
        starts = starts.iter().flat_map(|s| shifts.iter().map(move |&k| [s.as_slice(), &[k]].concat())).take(MAX_START_TURNS).collect();
    }
    let mut best: Option<Unwound> = None;
    let mut first_error: Option<CamError> = None;
    for start in starts {
        match unwind_from(machine, moves, previous, start) {
            Ok(u) => {
                if best.as_ref().is_none_or(|b| u.1.len() < b.1.len()) {
                    best = Some(u);
                }
            }
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    match (best, first_error) {
        (Some(u), _) => Ok(u),
        (None, Some(e)) => Err(e),
        (None, None) => unwind_from(machine, moves, previous, vec![0.0; n]),
    }
}

/// The moves of a continuous operation solved on [`turning_freely`]'s machine, brought within
/// `machine`'s limits by whole turns, and the rewinds they need (see [`Motion::Joints`]). A
/// rapid takes the equal angle within the limits nearest the one before it (it turns the
/// rotaries anyway); a cut that would go past a limit is split where it reaches it, and the rest
/// follows a rewind. The axes move evenly through each piece as before, so a split piece takes
/// its share of the time. Every angle is first moved by `shift` (whole turns, joint order).
fn unwind_from(machine: &Machine, moves: &[JointMove], previous: Option<&Joints>, mut shift: Vec<f64>) -> Result<Unwound> {
    shift.resize(machine.rotaries.len(), 0.0);
    let mut last: Option<Joints> = previous.cloned();
    let mut out = Vec::with_capacity(moves.len());
    let mut rewinds = Vec::new();
    for m in moves.iter().cloned() {
        let source = m.source();
        let at = |e: CamError| match e {
            CamError::Unsafe(s) => CamError::Unsafe(format!("move {source}: {s}")),
            other => other,
        };
        match m {
            JointMove::Dwell { .. } => out.push(m),
            JointMove::Rapid { mut to, pose, rotary_change, .. } => {
                for (a, s) in to.rotary.iter_mut().zip(&shift) {
                    *a += s;
                }
                snap_to_limits(machine, &mut to);
                for (i, lo, hi) in beyond(machine, &to) {
                    let reference = last.as_ref().and_then(|l| l.rotary.get(i).copied()).unwrap_or(0.0);
                    if let (Some(a), Some(s)) = (to.rotary.get_mut(i), shift.get_mut(i))
                        && let Some(t) = nearest_turn(*a, reference, Some((lo, hi)))
                    {
                        *s += t - *a;
                        *a = t;
                    }
                }
                check_limits(machine, &to).map_err(at)?;
                last = Some(to.clone());
                out.push(JointMove::Rapid { source, to, pose, rotary_change });
            }
            JointMove::Feed { mut to, pose, feed, mut minutes, .. } => {
                for (a, s) in to.rotary.iter_mut().zip(&shift) {
                    *a += s;
                }
                snap_to_limits(machine, &mut to);
                let Some(mut from) = last.clone() else { return Err(at(CamError::Unsafe("a cut before the first positioning move".into()))) };
                for _ in 0..MAX_REWINDS_PER_MOVE {
                    let over = beyond(machine, &to);
                    if over.is_empty() {
                        break;
                    }
                    // Up to where the first axis reaches its limit.
                    let mut t: f64 = 1.0;
                    for &(i, lo, hi) in &over {
                        let (a, b) = (from.rotary.get(i).copied().unwrap_or(0.0), to.rotary.get(i).copied().unwrap_or(0.0));
                        let limit = if b > hi { hi } else { lo };
                        if (b - a).abs() > 1e-12 {
                            t = t.min(((limit - a) / (b - a)).clamp(0.0, 1.0));
                        }
                    }
                    if t > 1e-9 {
                        let reached = from.lerp(&to, t);
                        out.push(JointMove::Feed { source, to: reached.clone(), pose: forward(machine, &reached), feed, minutes: minutes * t });
                        minutes *= 1.0 - t;
                        from = reached;
                    }
                    // Every axis past a limit turns back by whole turns, as far from the limit it
                    // heads for as the limits let it.
                    let mut back = from.rotary.clone();
                    for &(i, lo, hi) in &over {
                        let (Some(a), Some(b)) = (from.rotary.get(i).copied(), to.rotary.get(i).copied()) else { continue };
                        let turned =
                            if b > hi { a - TAU * ((a - lo + LIMIT_SLACK) / TAU).floor() } else { a + TAU * ((hi - a + LIMIT_SLACK) / TAU).floor() };
                        let delta = turned - a;
                        if delta.abs() < PI {
                            let letter = machine.rotaries.get(i).map_or('?', |r| r.letter.char());
                            return Err(at(CamError::Unsafe(format!(
                                "{letter} would turn past its limits at {:.3}°, with no room to turn it back a whole turn",
                                a.to_degrees()
                            ))));
                        }
                        if let (Some(x), Some(s), Some(y)) = (back.get_mut(i), shift.get_mut(i), to.rotary.get_mut(i)) {
                            *x = turned;
                            *s += delta;
                            *y += delta;
                        }
                    }
                    rewinds.push((out.len(), back.clone()));
                    from.rotary = back;
                }
                check_limits(machine, &to).map_err(at)?;
                last = Some(to.clone());
                out.push(JointMove::Feed { source, to, pose, feed, minutes });
            }
        }
        if out.len() > MAX_MOVES {
            return Err(CamError::TooLarge(format!("the machine moves would be more than {MAX_MOVES}")));
        }
    }
    Ok((out, rewinds))
}

/// Refuses a machine this writer has no output for yet: more than one rotary, or a rotary
/// lettered other than `A`.
pub(crate) fn four_axis_only(machine: &Machine) -> Result<()> {
    if machine.rotaries.len() > 1 {
        return Err(CamError::Unsupported(format!(
            "G-code for machine `{}`: output is written for at most one rotary axis (A), not {}",
            plain(&machine.name),
            machine.rotaries.len()
        )));
    }
    if let Some(r) = machine.rotaries.iter().find(|r| r.letter != AxisLetter::A) {
        return Err(CamError::Unsupported(format!(
            "G-code for machine `{}`: output is written for a rotary lettered A, not {}",
            plain(&machine.name),
            r.letter.char()
        )));
    }
    Ok(())
}

fn emit_ops(name: &str, work_offset: Option<u8>, ops: &[Op<'_>], post: &Post5, machine: &Machine) -> Result<(String, Vec<WrittenOperation>)> {
    post.validate()?;
    machine.validate()?;
    four_axis_only(machine)?;
    let base = &post.base;
    if ops.is_empty() {
        return invalid("the program has no operation");
    }
    if ops.len() > MAX_OPERATIONS {
        return Err(CamError::TooLarge(format!("more than {MAX_OPERATIONS} operations")));
    }
    let offset = work_offset.unwrap_or(1);
    // A post that leaves the offset to the operator can only mean the first one.
    if offset != 1 && !base.writes_work_offset() {
        return invalid(format!(
            "post `{}` writes no work offset, so the program would run in whatever offset the machine is in rather than {}",
            plain(&base.name),
            work_offset_code(offset)?
        ));
    }
    let opts = post.solve_options();
    // With rewinds, continuous operations are solved as if a rotary with room for a whole turn
    // turned without end, then brought back within its limits.
    let free = if post.rewind.is_some() { turning_freely(machine) } else { None };

    // Every operation solved first, so the program's highest clearance is known before the
    // first rotary turns.
    let mut planned: Vec<Planned<'_>> = Vec::with_capacity(ops.len());
    let mut chain: Option<Joints> = None;
    let mut total = 0usize;
    for op in ops {
        let (op_name, op_tool, op_rpm, op_coolant, op_plunge) = match op {
            Op::Continuous(o) => (o.name.as_str(), &o.tool, o.spindle_rpm, o.coolant, o.plunge_feed.or(o.tool.plunge_feed)),
            Op::Indexed(o) => (o.name.as_str(), &o.tool, o.spindle_rpm, o.coolant, None),
        };
        let label = plain(op_name);
        op_tool.validate()?;
        if !(op_rpm > 0.0) || !(op_rpm <= 1.0e6) {
            return invalid(format!("operation `{label}`: the spindle speed must be above 0"));
        }
        if op_plunge.is_some_and(|f| !feed_in_range(f)) {
            return invalid(format!("operation `{label}`: the plunge feed must be {MIN_FEED} to {MAX_FEED} mm/min"));
        }
        let motion = match op {
            Op::Indexed(o) => {
                frame_for_index(machine, &o.rotary).map_err(in_op(&label))?;
                if !head_upright(machine, &o.rotary) {
                    return Err(CamError::Unsupported(format!(
                        "operation `{label}`: indexed output with a tilted head (it needs a tilted work plane)"
                    )));
                }
                check_indexed(o, machine).map_err(in_op(&label))?;
                Motion::Indexed { rotary: o.rotary.clone(), path: o.path.moves.clone() }
            }
            Op::Continuous(o) => {
                verify5(&o.path, PI).map_err(in_op(&label))?;
                match &free {
                    Some(f) => {
                        let moves = solve_from(f, &o.path, &opts, chain.as_ref()).map_err(in_op(&label))?;
                        let (moves, rewinds) = unwind(machine, &moves, chain.as_ref()).map_err(in_op(&label))?;
                        Motion::Joints { moves, rewinds }
                    }
                    None => {
                        Motion::Joints { moves: solve_from(machine, &o.path, &opts, chain.as_ref()).map_err(in_op(&label))?, rewinds: Vec::new() }
                    }
                }
            }
        };
        total = total.saturating_add(match &motion {
            Motion::Joints { moves, .. } => moves.len(),
            Motion::Indexed { path, .. } => path.len(),
        });
        if total > MAX_MOVES {
            return Err(CamError::TooLarge(format!("the program would have more than {MAX_MOVES} moves")));
        }
        let (top, low, turned) = match &motion {
            Motion::Joints { moves, .. } => {
                if let Some(j) = moves.iter().rev().find_map(JointMove::joints) {
                    chain = Some(j.clone());
                }
                let height = |m: &JointMove| match m {
                    JointMove::Rapid { to, .. } | JointMove::Feed { to, .. } => Some(to.linear.z),
                    JointMove::Dwell { .. } => None,
                };
                let top = moves.iter().filter(|m| matches!(m, JointMove::Rapid { .. })).filter_map(height).reduce(f64::max);
                let turned = moves.iter().filter_map(JointMove::joints).any(|j| j.rotary.iter().any(|a| a.abs() > 1e-12));
                (top, moves.iter().filter_map(height).reduce(f64::min), turned)
            }
            Motion::Indexed { rotary, path } => {
                if let Some(to) = path.iter().rev().find_map(Move::target) {
                    chain = Some(Joints { linear: to, rotary: rotary.clone() });
                }
                let top = path
                    .iter()
                    .filter_map(|m| match m {
                        Move::Rapid { to } => Some(to.z),
                        _ => None,
                    })
                    .reduce(f64::max);
                (top, path.iter().filter_map(Move::target).map(|p| p.z).reduce(f64::min), rotary.iter().any(|a| a.abs() > 1e-12))
            }
        };
        planned.push(Planned { name: op_name, tool: op_tool, rpm: op_rpm, coolant: op_coolant, motion, top, low, turned, plunge: op_plunge });
    }
    // Where the tool's height is not known, it rises to the highest clearance of the program
    // (the post's, or any operation's highest rapid above it) before it moves across or a
    // rotary turns.
    let top = planned.iter().filter_map(|p| p.top).chain(post.clearance).reduce(f64::max);

    let mut order: Vec<(usize, char)> = machine.rotaries.iter().enumerate().map(|(i, r)| (i, r.letter.char())).collect();
    order.sort_by_key(|&(_, c)| c);
    let mut w = Writer5 {
        post: base,
        angle_decimals: post.angle_decimals,
        inverse_decimals: post.inverse_time_decimals,
        out: String::new(),
        line: base.line_numbers.map(|n| n.start),
        lin: [None, None, None],
        pos: None,
        height: Height::Unknown,
        rot: vec![None; machine.rotaries.len()],
        order,
        feed: None,
        inverse: false,
        mode: None,
        length_offset: None,
        length_on_z: base.tool_length_on_first_z,
        tool: None,
        held: Vec::new(),
        plane: ArcPlane::Xy,
    };
    let mut values = Values { program: plain(name), units: base.units.code().into(), work_offset: work_offset_code(offset)?, ..Values::default() };
    w.comment(name);
    if base.tool_list {
        // The lowest Z means something only while no rotary turns.
        let flat = planned.iter().all(|p| !p.turned);
        let mut tools: Vec<(&Tool, Option<f64>)> = Vec::new();
        for p in &planned {
            match tools.iter_mut().find(|(t, _)| t.number == p.tool.number) {
                Some((_, z)) => *z = (*z).into_iter().chain(p.low).reduce(f64::min),
                None => tools.push((p.tool, p.low)),
            }
        }
        for (t, z) in tools {
            w.comment(&tool_list_line(base, t, z.filter(|_| flat)));
        }
    }
    w.template(&base.header, &values)?;
    if let Some(lines) = base.safe_retract_at(|r| r.at_start) {
        w.safe_lines(lines, &values)?;
    }
    let mut tool: Option<u32> = None;
    let mut rpm: Option<String> = None;
    let mut coolant = false;
    let mut joints: Option<Joints> = None;
    let mut written: Vec<WrittenOperation> = Vec::with_capacity(planned.len());
    for p in planned {
        let mut report = WrittenOperation::default();
        let speed = number(p.rpm, 0);
        let new_tool = tool != Some(p.tool.number);
        // The blocks before the motion read any F per minute.
        if new_tool || rpm.as_ref() != Some(&speed) || p.coolant != coolant {
            w.per_minute();
        }
        // The spindle stops before the tool rises for a later tool change.
        if new_tool
            && tool.is_some()
            && let Some(lines) = base.safe_retract_at(|r| r.at_tool_change)
        {
            w.template(&base.spindle_off, &values)?;
            w.safe_lines(lines, &values)?;
        }
        if post.safe_rotate_first
            && let Some(rot) = first_turn(&p.motion)
            && w.turns(rot)
        {
            w.safe_lines(&post.safe_rotate, &values)?;
        }
        values.tool = p.tool.number.to_string();
        values.diameter = number(p.tool.diameter / base.units.mm(), base.decimals);
        values.rpm = speed.clone();
        w.blank();
        w.comment(&operation_comment(base, p.name, p.tool));
        if new_tool {
            if tool.is_some() || base.tool_change_first {
                // Coolant off while the tool is changed; the operation turns it back on.
                if coolant {
                    w.template(&base.coolant_off, &values)?;
                    coolant = false;
                }
                w.template(&base.tool_change, &values)?;
                if tool.is_some() && base.stops_at_tool_change() {
                    w.template(&["M5".to_string()], &values)?;
                    w.comment(&format!("change to tool {} diameter {}, set its length, then cycle start", values.tool, values.diameter));
                    w.template(&["M0".to_string()], &values)?;
                }
            }
            // A tool change stops the spindle and may leave the tool anywhere, except that one
            // made at the machine's top leaves it there.
            rpm = None;
            tool = Some(p.tool.number);
            let at_top = w.height == Height::Top;
            w.forget();
            if at_top {
                w.height = Height::Top;
            }
            w.tool = tool;
            if w.length_on_z {
                w.length_offset = tool;
            }
        }
        if rpm.as_ref() != Some(&speed) {
            w.template(&base.spindle_on, &values)?;
            rpm = Some(speed);
        }
        w.template(&base.operation_start, &values)?;
        if p.coolant != coolant {
            // With `coolant_after_turn` the coolant comes on after the operation's first turn
            // of the rotaries, where it starts with one.
            if p.coolant && post.coolant_after_turn && first_turn(&p.motion).is_some_and(|rot| w.turns(rot)) {
                w.hold(&base.coolant_on, &values)?;
            } else {
                w.template(if p.coolant { &base.coolant_on } else { &base.coolant_off }, &values)?;
            }
            coolant = p.coolant;
        }
        // With the same tool the tool is still where the last operation left it: only the
        // feed is stated again.
        w.feed = None;
        let clearance = if post.clearance_retract { post.clearance.or(p.top) } else { None };
        let turning = Turn { safe: &post.safe_rotate, clearance, top };
        match p.motion {
            Motion::Joints { moves, rewinds } => {
                let skip = link_steps(&moves, joints.as_ref(), post);
                let mut rewinds = rewinds.iter().peekable();
                for (k, m) in moves.iter().enumerate() {
                    while let Some((_, back)) = rewinds.next_if(|(at, _)| *at == k) {
                        let (Some(rewind), Some(from)) = (post.rewind.as_ref(), joints.clone()) else {
                            return Err(CamError::Unsafe(format!("operation `{}`: a rewind before the first positioning move", plain(p.name))));
                        };
                        // The operation's own clearance, or the post's where that is higher: never a turn
                        // lower than the operation's own links turn the rotaries.
                        let floor = clearance.into_iter().chain(p.top).reduce(f64::max);
                        let at = RewindAt { rewind, plunge_feed: p.plunge.unwrap_or(rewind.plunge_feed), floor, machine };
                        report.rewind_minutes += w.rewind(&from, back, &at, &turning, &values).map_err(in_op(&plain(p.name)))?;
                        report.rewinds += 1;
                        joints = Some(Joints { linear: from.linear, rotary: back.clone() });
                    }
                    if skip.get(k).copied().unwrap_or(false) {
                        if let Some(j) = m.joints() {
                            joints = Some(j.clone());
                        }
                        continue;
                    }
                    match m {
                        JointMove::Rapid { to, .. } => {
                            w.turn(&to.rotary, &post.safe_rotate, clearance, top, &values)?;
                            w.rapid(to.linear, top);
                        }
                        JointMove::Feed { to, feed, minutes, .. } => {
                            let minutes = minutes.max(MIN_MINUTES);
                            let rate = if !w.turns(&to.rotary) {
                                Rate::Linear(*feed)
                            } else if post.inverse_time {
                                Rate::Inverse(1.0 / minutes)
                            } else {
                                let from = joints.as_ref();
                                let len = from.map_or(0.0, |j| j.linear.dist(to.linear));
                                if len > 1e-9 {
                                    Rate::Linear(len / minutes)
                                } else {
                                    Rate::Degrees(from.map_or(0.0, |j| j.rotary_change(to)).to_degrees() / minutes)
                                }
                            };
                            w.feed(to.linear, &to.rotary, rate)?;
                        }
                        JointMove::Dwell { seconds, .. } => w.dwell(*seconds, &base.dwell, &mut values)?,
                    }
                    if let Some(j) = m.joints() {
                        joints = Some(j.clone());
                    }
                }
            }
            Motion::Indexed { rotary, path } => {
                if !path.is_empty() {
                    w.turn(&rotary, &post.safe_rotate, clearance, top, &values)?;
                }
                for m in &path {
                    match *m {
                        Move::Rapid { to } => w.rapid(to, top),
                        Move::Feed { to, feed } => w.feed(to, &rotary, Rate::Linear(feed))?,
                        Move::Arc { to, center, ccw, feed } => w.arc(to, center, ccw, feed, &rotary)?,
                        Move::PlaneArc { to, center, plane, ccw, feed } => w.plane_arc(to, center, plane, ccw, feed, &rotary)?,
                        Move::Dwell { seconds } => w.dwell(seconds, &base.dwell, &mut values)?,
                    }
                    if let Some(to) = m.target() {
                        joints = Some(Joints { linear: to, rotary: rotary.clone() });
                    }
                }
                w.plane_xy();
            }
        }
        w.release();
        w.template(&base.operation_end, &values)?;
        written.push(report);
    }
    w.per_minute();
    w.blank();
    if coolant {
        w.template(&base.coolant_off, &values)?;
    }
    w.template(&base.spindle_off, &values)?;
    if let Some(lines) = base.safe_retract_at(|r| r.at_end) {
        w.safe_lines(lines, &values)?;
    }
    w.template(&base.footer, &values)?;
    Ok((w.out, written))
}
/// One motion block of a 4th-axis program as read: where every axis is after it (mm and
/// radians, rotaries in the order of the letters asked for).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JointWords {
    Rapid {
        line: usize,
        linear: Vec3,
        rotary: Vec<f64>,
    },
    /// A cut. `f` is the `F` word in force: per minute (`G94`; the linear axes' feed in units
    /// of `mm_per_unit` mm, or degrees a minute for a move of the rotaries alone) or, with
    /// `inverse_time` (`G93`), 1 / the block's minutes.
    Feed {
        line: usize,
        linear: Vec3,
        rotary: Vec<f64>,
        f: f64,
        inverse_time: bool,
        mm_per_unit: f64,
    },
    /// An arc about `center` in the XY plane of the linear axes, the rotaries held, at `f` per minute.
    Arc {
        line: usize,
        linear: Vec3,
        center: Vec2,
        ccw: bool,
        rotary: Vec<f64>,
        f: f64,
        mm_per_unit: f64,
    },
    /// An arc in the XZ (`G18`) or YZ (`G19`) plane about `center` (its coordinates in the
    /// plane count), the rotaries held, at `f` per minute.
    PlaneArc {
        line: usize,
        linear: Vec3,
        center: Vec3,
        plane: ArcPlane,
        ccw: bool,
        rotary: Vec<f64>,
        f: f64,
        mm_per_unit: f64,
    },
    Dwell {
        line: usize,
        seconds: f64,
    },
}

impl JointWords {
    /// The block's line.
    pub fn line(&self) -> usize {
        match self {
            JointWords::Rapid { line, .. }
            | JointWords::Feed { line, .. }
            | JointWords::Arc { line, .. }
            | JointWords::PlaneArc { line, .. }
            | JointWords::Dwell { line, .. } => *line,
        }
    }

    /// The arc of an `Arc` or `PlaneArc` block that starts at `from`, in its plane.
    pub fn arc_from(&self, from: Vec3) -> Option<PlaneArc> {
        match *self {
            JointWords::Arc { linear, center, ccw, .. } => {
                Some(PlaneArc { from, to: linear, center: center.extend(from.z), plane: ArcPlane::Xy, ccw })
            }
            JointWords::PlaneArc { linear, center, plane, ccw, .. } => Some(PlaneArc { from, to: linear, center, plane, ccw }),
            _ => None,
        }
    }
}

/// Reads the motion of a 4th-axis program in absolute coordinates: `G0`–`G3` with `X Y Z`,
/// `I J K` (arcs in the plane `G17`, `G18` or `G19` selects, the rotaries held), the rotary
/// words `letters` (degrees) and `F`, `G93`/`G94`, `G20`/`G21`, `G4`, and `G91.1` (arc centres
/// from the arc's start; `G90.1` is refused). A `G53` block moves in machine coordinates and a
/// `G28`/`G30` block (as in `G28 G91 Z0.`) to the machine's home, so the axes they name are
/// unknown until written again. Any other move in incremental coordinates (`G91`) is refused,
/// and so is tool centre point control (`G43.4`), which this crate does not write yet.
/// Everything else (spindle, tools, coolant, offsets) is skipped, and so are blocks before
/// every axis is known.
pub fn parse5(text: &str, letters: &[AxisLetter]) -> Result<Vec<JointWords>> {
    let mut units = 1.0;
    let mut mode: Option<u8> = None;
    let mut inverse = false;
    let mut incremental = false;
    let mut feed: Option<f64> = None;
    let mut plane = ArcPlane::Xy;
    let mut lin: [Option<f64>; 3] = [None, None, None];
    let mut rot: Vec<Option<f64>> = vec![None; letters.len()];
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if n > MAX_LINES {
            return Err(CamError::TooLarge(format!("more than {MAX_LINES} lines")));
        }
        let ws = words(line, n)?;
        let (mut dwell, mut machine_coords, mut home, mut g91) = (false, false, false, false);
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
                280 | 300 => (machine_coords, home) = (true, true),
                434 => return Err(CamError::Parse(n, "not supported yet: tool centre point control (G43.4)".into())),
                530 => machine_coords = true,
                900 => incremental = false,
                910 => (incremental, g91) = (true, true),
                901 => return Err(CamError::Parse(n, "not supported yet: absolute arc centres (G90.1)".into())),
                930 => (inverse, feed) = (true, None),
                940 => (inverse, feed) = (false, None),
                _ => {}
            }
        }
        if g91 && !home {
            return Err(CamError::Parse(n, "not supported yet: incremental coordinates (G91) other than for a return home (G28 G91)".into()));
        }
        let word = |letter: char| ws.iter().find(|(c, _)| *c == letter).map(|(_, v)| *v);
        if dwell {
            out.push(JointWords::Dwell { line: n, seconds: word('X').or(word('P')).unwrap_or(0.0).max(0.0) });
            continue;
        }
        for c in ['A', 'B', 'C'] {
            if word(c).is_some() && !letters.iter().any(|l| l.char() == c) {
                return Err(CamError::Parse(n, format!("the machine has no {c} axis")));
            }
        }
        let f = word('F');
        if let Some(f) = f {
            if !(f > 0.0) {
                return Err(CamError::Parse(n, "a feed rate must be above 0".into()));
            }
            feed = Some(f);
        }
        let linear_words = [word('X'), word('Y'), word('Z')];
        let rotary_words: Vec<Option<f64>> = letters.iter().map(|l| word(l.char())).collect();
        if machine_coords {
            for (slot, w) in lin.iter_mut().zip(linear_words) {
                if w.is_some() {
                    *slot = None;
                }
            }
            for (slot, w) in rot.iter_mut().zip(&rotary_words) {
                if w.is_some() {
                    *slot = None;
                }
            }
            continue;
        }
        let (ti, tj, tk) = (word('I'), word('J'), word('K'));
        if incremental && !linear_words.iter().chain(&rotary_words).chain([&ti, &tj, &tk]).all(Option::is_none) {
            return Err(CamError::Parse(n, "not supported yet: incremental coordinates (G91)".into()));
        }
        if linear_words.iter().chain(&rotary_words).chain([&ti, &tj, &tk]).all(Option::is_none) {
            continue;
        }
        let Some(mode) = mode else { return Err(CamError::Parse(n, "coordinates before any G0, G1, G2 or G3".into())) };
        let from = match lin {
            [Some(x), Some(y), Some(z)] => Some(Vec3::new(x, y, z)),
            _ => None,
        };
        for (slot, w) in lin.iter_mut().zip(linear_words) {
            if let Some(v) = w {
                *slot = Some(v * units);
            }
        }
        for (slot, w) in rot.iter_mut().zip(&rotary_words) {
            if let Some(v) = w {
                *slot = Some(v.to_radians());
            }
        }
        let block_rate = if inverse { f } else { feed };
        if inverse {
            feed = None;
        }
        let ([Some(x), Some(y), Some(z)], Some(rotary)) = (lin, rot.iter().copied().collect::<Option<Vec<f64>>>()) else {
            if mode >= 2 {
                return Err(CamError::Parse(n, "an arc before the position is known".into()));
            }
            continue;
        };
        let linear = Vec3::new(x, y, z);
        if mode == 0 {
            out.push(JointWords::Rapid { line: n, linear, rotary });
            continue;
        }
        let Some(rate) = block_rate else {
            return Err(CamError::Parse(
                n,
                if inverse { "an inverse-time cut (G93) without its F".into() } else { "a cutting move without a feed rate".to_string() },
            ));
        };
        if mode == 1 {
            out.push(JointWords::Feed { line: n, linear, rotary, f: rate, inverse_time: inverse, mm_per_unit: units });
            continue;
        }
        if inverse {
            return Err(CamError::Parse(n, "not supported yet: arcs in inverse time (G93)".into()));
        }
        if rotary_words.iter().any(Option::is_some) {
            return Err(CamError::Parse(n, "not supported yet: an arc that turns a rotary".into()));
        }
        let Some(from) = from else { return Err(CamError::Parse(n, "an arc before the position is known".into())) };
        let offsets = [ti, tj, tk];
        let (u, v, _) = plane.axes();
        if offsets.get(u).copied().flatten().is_none() && offsets.get(v).copied().flatten().is_none() {
            let words = match plane {
                ArcPlane::Xy => "I and J",
                ArcPlane::Zx => "I and K (G18)",
                ArcPlane::Yz => "J and K (G19)",
            };
            return Err(CamError::Parse(n, format!("not supported yet: arcs without {words}")));
        }
        let off = |i: usize| offsets.get(i).copied().flatten().unwrap_or(0.0) * units;
        if plane == ArcPlane::Xy {
            let center = Vec2::new(from.x + off(0), from.y + off(1));
            out.push(JointWords::Arc { line: n, linear, center, ccw: mode == 3, rotary, f: rate, mm_per_unit: units });
        } else {
            // The centre's coordinate along the normal is the start's: only the plane's count.
            let mut center = from;
            for i in [u, v] {
                match i {
                    0 => center.x += off(0),
                    1 => center.y += off(1),
                    _ => center.z += off(2),
                }
            }
            out.push(JointWords::PlaneArc { line: n, linear, center, plane, ccw: mode == 3, rotary, f: rate, mm_per_unit: units });
        }
    }
    Ok(out)
}
