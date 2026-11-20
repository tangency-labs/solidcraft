//! Kinds of line (#4890 D2a): what a line on a drawing shows, and the width ISO 128-20 gives it in
//! a line group, so no writer or view chooses its own.
//!
//! - Widths come from the standard's series, each about √2 times the one before: 0.13, 0.18, 0.25,
//!   0.35, 0.5, 0.7, 1, 1.4 and 2 mm. A line group is named by its wide line; its narrow line is two
//!   steps down the series, half as wide (wide : narrow = 2 : 1).
//! - A visible edge is a wide line; a hidden edge, a centre line and a thin line (dimension,
//!   leader, a reference part's edge, hatching) are narrow. A hidden line is dashed, 12 line widths
//!   on and 3 off.

/// A line group, named by its wide line's width, and saved by it as text: `"0.25"` to `"2"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LineGroup {
    #[serde(rename = "0.25")]
    Mm0_25,
    #[serde(rename = "0.35")]
    Mm0_35,
    #[default]
    #[serde(rename = "0.5")]
    Mm0_5,
    #[serde(rename = "0.7")]
    Mm0_7,
    #[serde(rename = "1")]
    Mm1,
    #[serde(rename = "1.4")]
    Mm1_4,
    #[serde(rename = "2")]
    Mm2,
}

/// What a line shows. Saved by its name in lower case (`visible`), as a symbol's strokes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Visible,
    Hidden,
    Centre,
    Thin,
}

impl LineGroup {
    /// The group a drawing's lines are in until it chooses one.
    pub const DEFAULT: LineGroup = LineGroup::Mm0_5;

    /// The names a group is saved and asked for by, smallest first.
    pub const NAMES: [&'static str; 7] = ["0.25", "0.35", "0.5", "0.7", "1", "1.4", "2"];

    /// Its wide and narrow lines' widths, in millimetres.
    const fn widths_mm(self) -> (f64, f64) {
        match self {
            LineGroup::Mm0_25 => (0.25, 0.13),
            LineGroup::Mm0_35 => (0.35, 0.18),
            LineGroup::Mm0_5 => (0.5, 0.25),
            LineGroup::Mm0_7 => (0.7, 0.35),
            LineGroup::Mm1 => (1.0, 0.5),
            LineGroup::Mm1_4 => (1.4, 0.7),
            LineGroup::Mm2 => (2.0, 1.0),
        }
    }
}

impl LineKind {
    /// Its width in `group`: the wide line for a visible edge, the narrow one for the rest.
    pub const fn width_mm(self, group: LineGroup) -> f64 {
        let (wide, narrow) = group.widths_mm();
        match self {
            LineKind::Visible => wide,
            LineKind::Hidden | LineKind::Centre | LineKind::Thin => narrow,
        }
    }

    /// Its dash and gap in `group`, for a hidden line: ISO 128-20's dashed line, 12 and 3 line widths.
    /// The other kinds are solid until a writer draws them otherwise.
    pub fn dashes_mm(self, group: LineGroup) -> Option<[f64; 2]> {
        match self {
            LineKind::Hidden => {
                let d = self.width_mm(group);
                Some([12.0 * d, 3.0 * d])
            }
            LineKind::Visible | LineKind::Centre | LineKind::Thin => None,
        }
    }
}

#[cfg(test)]
#[path = "line_tests.rs"]
mod tests;
