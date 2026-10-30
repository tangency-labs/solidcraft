//! Kinds of line (#4890 D2a): what a line on a drawing shows, and the width ISO 128-20 gives it in
//! a line group, so no writer or view chooses its own.
//!
//! - Widths come from the standard's series, each about √2 times the one before: 0.13, 0.18, 0.25,
//!   0.35, 0.5, 0.7, 1, 1.4 and 2 mm. A line group is named by its wide line; its narrow line is two
//!   steps down the series, half as wide (wide : narrow = 2 : 1).
//! - A visible edge is a wide line; a hidden edge, a centre line and a thin line (dimension,
//!   leader, hatching) are narrow. Their dashes come with the hidden lines that first draw them
//!   (D2b).

/// A line group, named by its wide line's width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineGroup {
    Mm0_25,
    Mm0_35,
    Mm0_5,
    Mm0_7,
    Mm1,
    Mm1_4,
    Mm2,
}

/// What a line shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineKind {
    Visible,
    Hidden,
    Centre,
    Thin,
}

impl LineGroup {
    /// The group a drawing's lines are in until it chooses one.
    pub const DEFAULT: LineGroup = LineGroup::Mm0_5;

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
}

#[cfg(test)]
#[path = "line_tests.rs"]
mod tests;
