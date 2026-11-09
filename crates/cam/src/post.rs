//! Post processors: how a program is written for one kind of controller. A post is data, not
//! code: line templates around the motion blocks, and a few switches for how the motion itself
//! is written. The built-in ones are the JSON files in `crates/cam/posts/`, one per controller
//! (that folder's README says how to add one); a design can save its own.
//!
//! Templates may use `{program}` (program name), `{units}` (`G21` or `G20`), `{work_offset}`
//! (the setup's work offset, `G54` to `G59`), `{tool}` (tool number), `{diameter}` (tool
//! diameter in output units) and `{rpm}` (spindle speed); the dwell template uses `{seconds}` or
//! `{milliseconds}`.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::{CamError, Result, invalid};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineNumbers {
    pub start: u32,
    pub step: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Post {
    pub name: String,
    /// Other names `cam.post` takes for it (`linuxcnc` for `rs274`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// What controllers it is for and what it writes for them, for someone choosing a post.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
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
    /// The highest tool number the controller takes (inclusive); `None` for no limit. A program
    /// with a tool above it is refused before anything is written: a controller that rejects the
    /// `T` word does not stop the sender, so the lines after it would still run.
    #[serde(default)]
    pub max_tool: Option<u32>,
    /// The shortest straight cutting move the controller plans well (mm); 0 for no opinion. The
    /// writer changes nothing: it counts the moves shorter than this and reports them
    /// (`short_moves`), so a toolpath denser than the machine can use is seen before it runs.
    #[serde(default)]
    pub min_move: f64,
}

fn yes() -> bool {
    true
}

fn default_dwell() -> String {
    "G4 P{seconds}".into()
}

/// The built-in posts: one JSON file per controller in `crates/cam/posts/`, embedded in the
/// program. The names are the file names, in the order users see them.
const BUILTIN_FILES: &[(&str, &str)] = &[("rs274", include_str!("../posts/rs274.json")), ("tinyg", include_str!("../posts/tinyg.json"))];

/// Every built-in post, parsed once. A file that does not parse is left out here; the tests fail
/// on it (`every_builtin_post_is_a_file_that_parses_validates_and_posts_its_sample`).
fn builtins() -> &'static [Post] {
    static POSTS: OnceLock<Vec<Post>> = OnceLock::new();
    POSTS.get_or_init(|| BUILTIN_FILES.iter().filter_map(|(_, text)| serde_json::from_str::<Post>(text).ok()).collect())
}

impl Post {
    /// The built-in posts' names, as `cam.post` takes them: the files in `crates/cam/posts/`.
    pub const BUILTIN: &'static [&'static str] = &["rs274", "tinyg"];

    /// The built-in post called `name`, or one of its aliases (`linuxcnc` for `rs274`), in any case.
    pub fn builtin(name: &str) -> Option<Post> {
        let want = name.trim();
        builtins().iter().find(|p| p.name.eq_ignore_ascii_case(want) || p.aliases.iter().any(|a| a.eq_ignore_ascii_case(want))).cloned()
    }

    /// Whether the post writes the work offset (a `{work_offset}` in its header or tool change).
    pub fn writes_work_offset(&self) -> bool {
        self.header.iter().chain(&self.tool_change).any(|l| l.contains("{work_offset}"))
    }

    pub fn validate(&self) -> Result<()> {
        if self.description.chars().count() > 2000 || self.description.chars().any(|c| c.is_control() && c != '\n') {
            return invalid("the post's description is at most 2000 characters, without control characters");
        }
        let alias_ok = |a: &String| !a.is_empty() && a.len() <= 32 && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if self.aliases.len() > 8 || !self.aliases.iter().all(alias_ok) {
            return invalid("a post alias is 1 to 32 letters, digits, dashes or underscores, at most 8 of them");
        }
        if !self.min_move.is_finite() || !(0.0..=1.0).contains(&self.min_move) {
            return invalid("the post's minimum move is 0 (no limit) to 1 mm");
        }
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
        for block in [&self.header, &self.tool_change, &self.spindle_on, &self.spindle_off, &self.coolant_on, &self.coolant_off, &self.footer] {
            if block.len() > MAX_TEMPLATE_LINES {
                return Err(CamError::TooLarge(format!("a post template has more than {MAX_TEMPLATE_LINES} lines")));
            }
            for line in block {
                if line.len() > MAX_TEMPLATE_LEN || line.chars().any(char::is_control) {
                    return invalid("a post template line is too long or has control characters");
                }
                // Catches unknown placeholders before any program is written.
                fill(line, &Values::default())?;
            }
        }
        Ok(())
    }
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
