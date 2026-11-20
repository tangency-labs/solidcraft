//! Semantic dimension annotations and text formatting, independent of layout, UI language and saved data.

use serde::{Deserialize, Serialize};

use crate::display_units::{DecimalMarker, MAX_DECIMALS, NumberFormat, Shown, format_number, format_value, grouped_digits};
use crate::{DocError, Result, expr};

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

/// A drafting profile keeps it, so it is saved with the profile (W09d).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// The unrounded measurement and semantic annotations of one dimension.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionValue {
    pub nominal: f64,
    pub kind: expr::Kind,
    pub tolerance: Tolerance,
    pub presentation: TolerancePresentation,
    pub role: Role,
    pub indicator: Option<Indicator>,
    pub count: u32,
    pub not_to_scale: bool,
}

/// Drawing-owned presentation; independent of the user's UI locale.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionStyle {
    pub unit: String,
    pub number: NumberFormat,
    pub decimals: u8,
    /// Maximum precision of both stacked values. Trailing-zero suppression removes
    /// only shared zeros, so their decimal places match (except an unsigned zero deviation).
    pub tolerance_decimals: u8,
    pub repeat: Repeat,
}

/// Typed runs and layout metadata, with a single-line form for headless consumers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DimensionText {
    pub count: Option<String>,
    pub indicator: Option<String>,
    /// Retained for inspection; limits replace this run in both plain text and layout.
    pub nominal: String,
    pub tolerance: ToleranceText,
    pub frame: Frame,
    pub underline: bool,
    pub warnings: Vec<DimensionWarning>,
}

impl DimensionText {
    /// Limits replace the nominal; boxes and underlines remain layout metadata.
    pub fn plain(&self) -> String {
        let mut text = String::new();
        if self.frame == Frame::Parentheses {
            text.push('(');
        }
        if let Some(count) = &self.count {
            text.push_str(count);
        }
        if let Some(indicator) = &self.indicator {
            text.push_str(indicator);
        }
        if !matches!(&self.tolerance, ToleranceText::Limits { .. }) {
            text.push_str(&self.nominal);
        }
        match &self.tolerance {
            ToleranceText::None => {}
            ToleranceText::Symmetric(t) | ToleranceText::Code(t) => {
                text.push(' ');
                text.push_str(t);
            }
            ToleranceText::Stacked { upper, lower } => {
                text.push(' ');
                text.push_str(upper);
                text.push('/');
                text.push_str(lower);
            }
            ToleranceText::Limits { upper, lower } => {
                text.push_str(upper);
                text.push('/');
                if let Some(indicator) = &self.indicator {
                    text.push_str(indicator);
                }
                text.push_str(lower);
            }
        }
        if self.frame == Frame::Parentheses {
            text.push(')');
        }
        text
    }
}

/// Format a dimension without changing its value or dropping incompatible annotations.
pub fn format_dimension(value: &DimensionValue, style: &DimensionStyle) -> Result<DimensionText> {
    if !value.nominal.is_finite() {
        return Err(DocError::Invalid("dimension nominal must be finite".into()));
    }
    if style.decimals > MAX_DECIMALS {
        return Err(DocError::Invalid(format!("dimension decimals must be at most {MAX_DECIMALS}")));
    }
    if style.tolerance_decimals > MAX_DECIMALS {
        return Err(DocError::Invalid(format!("dimension tolerance_decimals must be at most {MAX_DECIMALS}")));
    }
    if !(1..=10_000).contains(&value.count) {
        return Err(DocError::Invalid("dimension count must be between 1 and 10000".into()));
    }
    if value.role != Role::Normal && !matches!(&value.tolerance, Tolerance::None) {
        return Err(DocError::Invalid("auxiliary and exact dimension roles cannot have a tolerance".into()));
    }
    value.tolerance.validate()?;
    let shown = format_value(value.nominal, value.kind, &style.unit, style.decimals, &style.number)?;
    let tolerance = match &value.tolerance {
        Tolerance::None => ToleranceText::None,
        Tolerance::Fit { class } => ToleranceText::Code(class.clone()),
        t if value.presentation == TolerancePresentation::Limits => {
            let (upper, lower) = t.deviations().ok_or_else(|| DocError::Invalid("limits require numeric deviations".into()))?;
            let (upper, lower) = dimension_pair(value.nominal + upper, value.nominal + lower, value.kind, style)?;
            ToleranceText::Limits { upper: upper.text, lower: lower.text }
        }
        Tolerance::Symmetric { deviation } => {
            let d = dimension_deviation(*deviation, value.kind, style)?;
            ToleranceText::Symmetric(format!("±{}", d.text))
        }
        Tolerance::Deviations { upper, lower } => {
            let (upper, lower) = dimension_pair(*upper, *lower, value.kind, style)?;
            ToleranceText::Stacked { upper: signed_deviation(upper), lower: signed_deviation(lower) }
        }
    };
    let mut warnings = Vec::new();
    if let Some((upper, lower)) = value.tolerance.deviations() {
        // Shown.value is in display units. Centre the interval on the shown nominal,
        // in internal units. Compare endpoints directly: subtracting the nominal can
        // manufacture an outside offset at an inclusive decimal boundary (1.1 - 1).
        let (_, scale) = expr::unit_info(&style.unit).ok_or_else(|| DocError::Invalid("unknown number display unit".into()))?;
        let centre = shown.value * scale;
        // A non-identity unit conversion can put an unchanged decimal nominal
        // a few floating-point steps from its original value. Account for both
        // conversions here, never an absolute margin or slack at the tolerance bounds.
        let conversion_roundoff = if scale == 1.0 { 0.0 } else { 2.0 * f64::EPSILON * centre.abs().max(value.nominal.abs()) };
        let moved = (centre - value.nominal).abs() > conversion_roundoff;
        if moved && !(centre + lower..=centre + upper).contains(&value.nominal) {
            warnings.push(DimensionWarning::RoundingOutsideTolerance);
        }
    }
    let count = if value.count > 1 {
        let repeat = match style.repeat {
            Repeat::Times => '×',
            Repeat::X => 'X',
        };
        Some(format!("{}{repeat} ", format_number(f64::from(value.count), 0, &style.number)?.text))
    } else {
        None
    };
    Ok(DimensionText {
        count,
        indicator: value.indicator.map(|i| {
            match i {
                Indicator::Diameter => "Ø",
                Indicator::Radius => "R",
                Indicator::Square => "□",
                Indicator::SphericalDiameter => "SØ",
                Indicator::SphericalRadius => "SR",
            }
            .into()
        }),
        nominal: shown.text,
        tolerance,
        frame: match value.role {
            Role::Normal => Frame::None,
            Role::Auxiliary => Frame::Parentheses,
            Role::Exact => Frame::Box,
        },
        underline: value.not_to_scale,
        warnings,
    })
}

fn dimension_deviation(deviation: f64, kind: expr::Kind, style: &DimensionStyle) -> Result<Shown> {
    let mut shown = format_value(deviation, kind, &style.unit, style.tolerance_decimals, &style.number)?;
    if shown.value == 0.0 {
        shown.text = "0".into();
    }
    Ok(shown)
}

/// Pad already-rounded fractions to their shared precision without rounding again.
fn dimension_pair(upper: f64, lower: f64, kind: expr::Kind, style: &DimensionStyle) -> Result<(Shown, Shown)> {
    let mut upper = format_value(upper, kind, &style.unit, style.tolerance_decimals, &style.number)?;
    let mut lower = format_value(lower, kind, &style.unit, style.tolerance_decimals, &style.number)?;
    let marker = match style.number.marker {
        DecimalMarker::Point => '.',
        DecimalMarker::Comma => ',',
    };
    let places = |text: &str| text.split_once(marker).map_or(0, |(_, fraction)| fraction.chars().filter(char::is_ascii_digit).count());
    let precision = places(&upper.text).max(places(&lower.text));
    for shown in [&mut upper, &mut lower] {
        if places(&shown.text) == precision {
            continue;
        }
        let (integer, fraction) = shown.text.split_once(marker).unwrap_or((&shown.text, ""));
        let mut digits: String = fraction.chars().filter(char::is_ascii_digit).collect();
        // W09b has already bounded precision to at most eight decimal places.
        digits.extend(std::iter::repeat_n('0', precision - digits.len()));
        let mut text = if integer == "0" && !style.number.leading_zero { String::new() } else { integer.to_owned() };
        text.push(marker);
        grouped_digits(&mut text, &digits, false, style.number.groups);
        shown.text = text;
    }
    Ok((upper, lower))
}

fn signed_deviation(shown: Shown) -> String {
    if shown.value == 0.0 {
        "0".into()
    } else if shown.value > 0.0 {
        format!("+{}", shown.text)
    } else {
        shown.text
    }
}
