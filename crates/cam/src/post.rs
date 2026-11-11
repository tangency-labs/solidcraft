//! Post processors: how a program is written for one kind of controller. A post is data (it
//! can be read from JSON), not code: line templates around the motion blocks, and a few
//! switches for how the motion itself is written.
//!
//! Templates may use `{program}` (program name), `{units}` (`G21` or `G20`), `{work_offset}`
//! (the setup's work offset, `G54` to `G59`), `{tool}` (tool number), `{diameter}` (tool
//! diameter in output units) and `{rpm}` (spindle speed); the dwell template uses `{seconds}` or
//! `{milliseconds}`. No template may switch inverse-time feeds on (`G93`): the writer would not
//! know it was in force.
//!
//! The switches for a program's layout (every one off by default, which is how the built-in
//! posts write): a tool list after the program's name, comments in capitals from a set of
//! characters, a blank line before each operation and before the end, the operation's name
//! alone as its comment, lines at the start and end of every operation, a safe retract to the
//! machine's top ([`SafeRetract`]) at the start, before each later tool change and at the end,
//! the tool length offset set with the first Z move after a tool change (`G43 Z15. H1`), and
//! motion codes written only when the motion mode changes.

use serde::{Deserialize, Serialize};

use crate::gcode::{MAX_COMMENT, plain, words};
use crate::{CamError, Result, invalid};

/// Most characters a post may allow in comments besides letters and digits.
const MAX_COMMENT_CHARS: usize = 64;
/// Most lines in one template block.
const MAX_TEMPLATE_LINES: usize = 64;
/// Longest template line.
const MAX_TEMPLATE_LEN: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    Mm,
    Inch,
}

impl Units {
    /// Millimetres per output unit.
    pub fn mm(self) -> f64 {
        match self {
            Units::Mm => 1.0,
            Units::Inch => 25.4,
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Units::Mm => "G21",
            Units::Inch => "G20",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentStyle {
    /// `(comment)`
    Parentheses,
    /// `; comment`
    Semicolon,
    /// No comments are written.
    None,
}

/// Lines that take the tool straight up to the machine's top (e.g. `G28 G91 Z0.` then `G90`,
/// or `G53 G0 Z0`), and where they are written. After them the writer knows the tool is above
/// everything: the next rapid goes across first and down last, stating every axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SafeRetract {
    pub lines: Vec<String>,
    /// After the header, before the first operation.
    #[serde(default)]
    pub at_start: bool,
    /// Before each tool change after the first, with the spindle stopped (the post's
    /// `spindle_off`) before it rises. A tool change is taken to leave the tool at the top.
    #[serde(default)]
    pub at_tool_change: bool,
    /// At the end, after the spindle stops and before the footer.
    #[serde(default)]
    pub at_end: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineNumbers {
    pub start: u32,
    pub step: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Post {
    pub name: String,
    /// File extension without the dot.
    pub extension: String,
    pub units: Units,
    /// Decimals of coordinates (at most 6).
    pub decimals: u8,
    /// Decimals of feed rates (at most 4).
    pub feed_decimals: u8,
    #[serde(default)]
    pub line_numbers: Option<LineNumbers>,
    pub comments: CommentStyle,
    /// Written once at the start.
    pub header: Vec<String>,
    /// Written before the first operation of each tool.
    pub tool_change: Vec<String>,
    /// Whether `tool_change` is also written for the program's first tool.
    pub tool_change_first: bool,
    /// Whether every tool change after the program's first tool is followed by a programmed
    /// stop: the spindle off (`M5`), a comment naming the tool, and `M0`, for a controller that
    /// ignores `M6` (the operator changes the tool, sets its length and presses Cycle Start). Not
    /// written when `tool_change` itself stops (`M0`, `M1`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub stop_at_tool_change: bool,
    pub spindle_on: Vec<String>,
    pub spindle_off: Vec<String>,
    pub coolant_on: Vec<String>,
    pub coolant_off: Vec<String>,
    /// Written once at the end.
    pub footer: Vec<String>,
    /// Whether arcs are written as `G2`/`G3` with `I` `J`; otherwise as short lines.
    pub arcs: bool,
    /// Chord error of arcs written as lines (mm).
    pub arc_tolerance: f64,
    /// Whether an arc that also moves in Z (a helix, as ramps make) is written as an arc; otherwise as short lines.
    #[serde(default = "yes")]
    pub helical_arcs: bool,
    /// Whether whole numbers are written with a decimal point (`X10.`), for controllers that
    /// read `X10` as ten of their smallest unit.
    #[serde(default)]
    pub trailing_decimal: bool,
    /// Whether motion codes are written with two digits (`G01` rather than `G1`).
    #[serde(default)]
    pub two_digit_codes: bool,
    /// How a dwell is written: `{seconds}` or `{milliseconds}` (a whole number) stands for its length.
    #[serde(default = "default_dwell")]
    pub dwell: String,
    /// Whether comments are written in capitals.
    #[serde(default, skip_serializing_if = "is_false")]
    pub upper_case_comments: bool,
    /// The only characters other than ASCII letters and digits a comment keeps (others are left
    /// out), for controllers that read only some; `None` for the default set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_chars: Option<String>,
    /// Whether the program's name is followed by one comment per tool, in the order they are
    /// first used: number, diameter, corner radius, lowest Z of its operations (left out when a
    /// rotary turns) and kind, as in `T1 D=10. CR=0.2 - ZMIN=-0.05 - FLAT END MILL`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub tool_list: bool,
    /// Whether an operation's comment is its name alone, rather than its name and tool.
    #[serde(default, skip_serializing_if = "is_false")]
    pub operation_name_only: bool,
    /// Whether a blank line comes before every operation and before the end of the program.
    #[serde(default, skip_serializing_if = "is_false")]
    pub blank_lines: bool,
    /// Written at the start of every operation, after its tool change and spindle and before its
    /// coolant (e.g. `G17 G90 G94` and `{work_offset}`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operation_start: Vec<String>,
    /// Written after the motion of every operation (e.g. `G17`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operation_end: Vec<String>,
    /// The retract to the machine's top, and where it is written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_retract: Option<SafeRetract>,
    /// Whether the tool length offset is set with the first Z move after a tool change or a safe
    /// retract (`G43 Z15. H1`), rather than by the tool change lines.
    #[serde(default, skip_serializing_if = "is_false")]
    pub tool_length_on_first_z: bool,
    /// Whether a motion code (`G0` to `G3`) is written only when the motion mode changes: `Z0.25
    /// F50.` after a `G1` block. Every template line makes the next motion state its code again.
    /// Arcs then leave out an end `X` or `Y` that does not change, as lines always do.
    #[serde(default, skip_serializing_if = "is_false")]
    pub modal_motion: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn yes() -> bool {
    true
}

fn default_dwell() -> String {
    "G4 P{seconds}".into()
}

fn lines(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

impl Post {
    /// Whether a programmed stop follows a tool change after the first ([`Post::stop_at_tool_change`]).
    pub fn stops_at_tool_change(&self) -> bool {
        let stops = |l: &String| l.split_whitespace().any(|w| ["M0", "M00", "M1", "M01"].iter().any(|m| w.eq_ignore_ascii_case(m)));
        self.stop_at_tool_change && !self.tool_change.iter().any(stops)
    }

    /// Controllers that follow the NIST RS-274/NGC language closely (LinuxCNC and relatives):
    /// automatic tool change with tool length offsets.
    pub fn rs274() -> Post {
        Post {
            name: "rs274".into(),
            extension: "ngc".into(),
            units: Units::Mm,
            decimals: 4,
            feed_decimals: 1,
            line_numbers: None,
            comments: CommentStyle::Parentheses,
            header: lines(&["G17 {units} G40 G49 {work_offset} G80 G90 G94"]),
            tool_change: lines(&["M5", "T{tool} M6", "G43 H{tool}"]),
            tool_change_first: true,
            stop_at_tool_change: false,
            spindle_on: lines(&["S{rpm} M3"]),
            spindle_off: lines(&["M5"]),
            coolant_on: lines(&["M8"]),
            coolant_off: lines(&["M9"]),
            footer: lines(&["M30"]),
            arcs: true,
            arc_tolerance: 0.005,
            helical_arcs: true,
            trailing_decimal: false,
            two_digit_codes: false,
            dwell: default_dwell(),
            upper_case_comments: false,
            comment_chars: None,
            tool_list: false,
            operation_name_only: false,
            blank_lines: false,
            operation_start: Vec::new(),
            operation_end: Vec::new(),
            safe_retract: None,
            tool_length_on_first_z: false,
            modal_motion: false,
        }
    }

    /// The built-in post called `name`.
    pub fn builtin(name: &str) -> Option<Post> {
        match name.trim().to_ascii_lowercase().as_str() {
            "rs274" | "linuxcnc" => Some(Post::rs274()),
            _ => None,
        }
    }

    pub const BUILTIN: &'static [&'static str] = &["rs274"];

    /// Whether the post writes the work offset (a `{work_offset}` in its header, tool change or
    /// operation start).
    pub fn writes_work_offset(&self) -> bool {
        self.header.iter().chain(&self.tool_change).chain(&self.operation_start).any(|l| l.contains("{work_offset}"))
    }

    /// The safe retract lines, if the post writes them at the place `at` picks.
    pub(crate) fn safe_retract_at(&self, at: fn(&SafeRetract) -> bool) -> Option<&[String]> {
        self.safe_retract.as_ref().filter(|r| at(r)).map(|r| r.lines.as_slice())
    }

    /// `text` as a comment of this post holds it: in capitals if the post says so, and only its
    /// characters, at most [`MAX_COMMENT`] of them.
    pub(crate) fn comment_text(&self, text: &str) -> String {
        let text = if self.upper_case_comments { text.to_ascii_uppercase() } else { text.to_string() };
        match &self.comment_chars {
            None => plain(&text),
            Some(allowed) => {
                text.chars().filter(|c| c.is_ascii_alphanumeric() || allowed.contains(*c)).take(MAX_COMMENT).collect::<String>().trim().to_string()
            }
        }
    }

    /// `text` as a whole comment line of this post (`(TEXT)` or `; TEXT`); `None` when the post
    /// writes no comments.
    pub fn comment_line(&self, text: &str) -> Option<String> {
        let text = self.comment_text(text);
        match self.comments {
            CommentStyle::Parentheses => Some(format!("({text})")),
            CommentStyle::Semicolon => Some(format!("; {text}")),
            CommentStyle::None => None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.decimals > 6 || self.feed_decimals > 4 {
            return invalid("a post writes at most 6 decimals for coordinates and 4 for feeds");
        }
        if !(self.arc_tolerance > 0.0) || !self.arc_tolerance.is_finite() {
            return invalid("the post's arc tolerance must be above 0");
        }
        if self.line_numbers.is_some_and(|n| n.step == 0) {
            return invalid("the post's line number step must be above 0");
        }
        if self.extension.is_empty() || self.extension.len() > 8 || !self.extension.chars().all(|c| c.is_ascii_alphanumeric()) {
            return invalid("the post's file extension must be 1 to 8 letters or digits");
        }
        if self.dwell.len() > MAX_TEMPLATE_LEN || self.dwell.chars().any(char::is_control) {
            return invalid("the post's dwell template is too long or has control characters");
        }
        fill(&self.dwell, &Values::default())?;
        crate::machine_time::DwellUnit::of(self)?;
        for block in [
            &self.header,
            &self.tool_change,
            &self.spindle_on,
            &self.spindle_off,
            &self.coolant_on,
            &self.coolant_off,
            &self.footer,
            &self.operation_start,
            &self.operation_end,
        ] {
            check_template(block)?;
        }
        if let Some(r) = &self.safe_retract {
            check_template(&r.lines)?;
            if r.lines.is_empty() && (r.at_start || r.at_tool_change || r.at_end) {
                return invalid("the post's safe retract has no lines");
            }
        }
        if let Some(chars) = &self.comment_chars
            && (chars.chars().count() > MAX_COMMENT_CHARS || chars.chars().any(|c| !c.is_ascii() || c.is_ascii_control() || "();%".contains(c)))
        {
            return invalid(format!(
                "the post's comment characters must be at most {MAX_COMMENT_CHARS} printable ASCII characters other than ( ) ; %"
            ));
        }
        Ok(())
    }
}

/// Checks a template block: not too many or too long lines, no control characters, only known placeholders.
pub(crate) fn check_template(block: &[String]) -> Result<()> {
    if block.len() > MAX_TEMPLATE_LINES {
        return Err(CamError::TooLarge(format!("a post template has more than {MAX_TEMPLATE_LINES} lines")));
    }
    for line in block {
        if line.len() > MAX_TEMPLATE_LEN || line.chars().any(char::is_control) {
            return invalid("a post template line is too long or has control characters");
        }
        // Catches unknown placeholders before any program is written.
        let filled = fill(line, &Values::default())?;
        // A line the reader cannot take apart (a placeholder left empty, a free-form message)
        // holds no feed mode the writer must follow.
        if words(&filled, 0).is_ok_and(|ws| ws.iter().any(|&(c, v)| c == 'G' && (v * 10.0).round() as i64 == 930)) {
            return invalid("a post template may not switch inverse-time feeds on (G93)");
        }
    }
    Ok(())
}

/// What the placeholders stand for.
#[derive(Clone, Debug, Default)]
pub(crate) struct Values {
    pub program: String,
    pub units: String,
    pub work_offset: String,
    pub tool: String,
    pub diameter: String,
    pub rpm: String,
    pub seconds: String,
    pub milliseconds: String,
}

/// `line` with its placeholders replaced.
pub(crate) fn fill(line: &str, v: &Values) -> Result<String> {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('{') {
        let (before, after) = rest.split_at(open);
        out.push_str(before);
        let Some(close) = after.find('}') else { return invalid(format!("post template `{line}`: `{{` without `}}`")) };
        let (name, tail) = after.split_at(close);
        out.push_str(match name.strip_prefix('{').unwrap_or(name) {
            "program" => &v.program,
            "units" => &v.units,
            "work_offset" => &v.work_offset,
            "tool" => &v.tool,
            "diameter" => &v.diameter,
            "rpm" => &v.rpm,
            "seconds" => &v.seconds,
            "milliseconds" => &v.milliseconds,
            other => return invalid(format!("post template `{line}`: unknown placeholder `{{{other}}}`")),
        });
        rest = tail.strip_prefix('}').unwrap_or(tail);
    }
    out.push_str(rest);
    Ok(out)
}
