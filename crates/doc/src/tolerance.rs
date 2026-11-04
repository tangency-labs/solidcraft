//! Semantic dimension annotations, independent of layout, UI language and saved data.

use crate::{DocError, Result};

/// Deviations are offsets from the unrounded nominal, in internal units (mm or rad).
#[derive(Clone, Debug, PartialEq)]
pub enum Tolerance {
    None,
    Symmetric {
        deviation: f64,
    },
    Deviations {
        upper: f64,
        lower: f64,
    },
    /// A fit code only; no ISO 286 deviation table is supplied.
    Fit {
        class: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TolerancePresentation {
    Deviations,
    Limits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Normal,
    Auxiliary,
    Exact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indicator {
    Diameter,
    Radius,
    Square,
    SphericalDiameter,
    SphericalRadius,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Times,
    X,
}

/// Text runs for a future paper layout; the symmetric run already includes its ± sign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToleranceText {
    None,
    Symmetric(String),
    /// Signed deviations beside the nominal; zero is unsigned and has no decimals.
    Stacked {
        upper: String,
        lower: String,
    },
    /// Absolute bounds that replace the nominal in the displayed dimension.
    Limits {
        upper: String,
        lower: String,
    },
    Code(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    None,
    Box,
    Parentheses,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimensionWarning {
    /// Rounding moved the nominal, leaving the unrounded value outside the bounds
    /// centred on the rounded nominal. An unchanged nominal never raises this warning.
    RoundingOutsideTolerance,
}

impl Tolerance {
    pub(crate) fn validate(&self) -> Result<()> {
        let deviation = |v: f64, name: &str| {
            if !v.is_finite() {
                Err(DocError::Invalid(format!("{name} deviation must be finite")))
            } else if v.abs() > 1e6 {
                Err(DocError::Invalid(format!("{name} deviation magnitude must be at most 1e6")))
            } else {
                Ok(())
            }
        };
        match self {
            Self::None => Ok(()),
            Self::Symmetric { deviation: d } => {
                deviation(*d, "symmetric")?;
                if *d <= 0.0 {
                    return Err(DocError::Invalid("symmetric deviation must be greater than zero".into()));
                }
                Ok(())
            }
            Self::Deviations { upper, lower } => {
                deviation(*upper, "upper")?;
                deviation(*lower, "lower")?;
                if upper <= lower {
                    return Err(DocError::Invalid("upper deviation must be greater than lower deviation".into()));
                }
                Ok(())
            }
            Self::Fit { class } if valid_fit(class) => Ok(()),
            Self::Fit { .. } => {
                Err(DocError::Invalid("fit class must have a permitted all-upper-case or all-lower-case letter code and grade 01, 0 or 1–18".into()))
            }
        }
    }

    pub(crate) fn deviations(&self) -> Option<(f64, f64)> {
        match *self {
            Self::Symmetric { deviation } => Some((deviation, -deviation)),
            Self::Deviations { upper, lower } => Some((upper, lower)),
            Self::None | Self::Fit { .. } => None,
        }
    }
}

fn valid_fit(class: &str) -> bool {
    // The longest accepted code is two ASCII letters and a two-digit grade.
    // Refuse hostile strings before allocating or scanning their contents.
    if class.len() > 4 {
        return false;
    }
    let code = class.trim_end_matches(|c: char| c.is_ascii_digit());
    let grade = class.get(code.len()..).unwrap_or_default();
    let upper = code.to_ascii_uppercase();
    let uniform_case = code == upper || code == code.to_ascii_lowercase();
    uniform_case
        && matches!(
            upper.as_str(),
            "A" | "B"
                | "C"
                | "CD"
                | "D"
                | "E"
                | "EF"
                | "F"
                | "FG"
                | "G"
                | "H"
                | "J"
                | "JS"
                | "K"
                | "M"
                | "N"
                | "P"
                | "R"
                | "S"
                | "T"
                | "U"
                | "V"
                | "X"
                | "Y"
                | "Z"
                | "ZA"
                | "ZB"
                | "ZC"
        )
        && matches!(
            grade,
            "01" | "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "10" | "11" | "12" | "13" | "14" | "15" | "16" | "17" | "18"
        )
}
