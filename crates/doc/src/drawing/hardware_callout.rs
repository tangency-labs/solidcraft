//! A hardware callout (#20112): a note whose text reads a press-in hardware record, "4× FLUSH NUT M5"
//! or "2× STUD M4 × 10", so it follows the record when the model changes.
//!
//! - **The reference** ([`HardwareRef`]) names a Hole feature that carries hardware and one of its
//!   holes by its place among the feature's holes (a pattern or a mirror makes copies), by persistent ids
//!   only. It is where the callout's leader ends on a flat pattern view; on a folded view the leader
//!   ends where the note's anchor points.
//! - **The reading** ([`read`]) is what the evaluated sheets say now: the hardware's kind, thread and
//!   length, and how many holes of that hardware the sheet still has. No hole left (the feature is
//!   gone, suppressed, changed to a plain hole, or a later feature cut the hole away), or none at the
//!   place, is lost: the callout keeps its saved text and is never moved to another hole.
//! - **The text** ([`text`]) is written with the hole callout's style ([`super::hole_callout::TextStyle`]):
//!   its count form, its length formatter and its capitals, so a tapped hole and a hardware hole read alike.

use serde::{Deserialize, Serialize};

use super::hole_callout::TextStyle;
use crate::hardware_catalog::HardwareKind;
use crate::{DocError, Result, SurfaceSheet, display_units};

/// The most holes a hardware reference may count in (the feature's own limit, as a hole callout's).
pub const MAX_COPIES: u32 = 10_000;

/// One press-in hardware hole, by persistent ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareRef {
    /// The Hole feature's id.
    pub feature: u64,
    /// Which of the feature's holes, from 0, in the order the flat pattern lists them.
    #[serde(default)]
    pub copy: u32,
}

impl HardwareRef {
    /// Refuses a copy number no feature could have.
    pub fn validate(&self) -> Result<()> {
        if self.copy >= MAX_COPIES {
            return Err(DocError::Invalid(format!("a hardware callout's copy is 0 to {}", MAX_COPIES - 1)));
        }
        Ok(())
    }
}

/// What the sheets say of a hardware reference now.
#[derive(Clone, Debug, PartialEq)]
pub struct Hardware {
    /// The sheet metal body the hole is in.
    pub body: String,
    pub kind: HardwareKind,
    /// The thread as written (`M5`).
    pub thread: String,
    /// A stud's or standoff's length (mm); none for a nut.
    pub length: Option<f64>,
    /// How many holes of this hardware its sheet has.
    pub count: usize,
}

/// What the evaluated `sheets` say of `hole`, or why it is lost. The count is every hole of the same
/// sheet with the same kind, thread and length: a pattern's or a mirror's copies are holes of their own
/// feature, and a callout reads "4×" for all the hardware a shop fits alike.
pub fn read(sheets: &[SurfaceSheet], hole: &HardwareRef) -> std::result::Result<Hardware, String> {
    let copy = usize::try_from(hole.copy).unwrap_or(usize::MAX);
    let mut any = false;
    for sheet in sheets {
        let holes = sheet.hardware_holes();
        let mut own = holes.iter().filter(|h| h.feature == hole.feature);
        any |= own.clone().next().is_some();
        if let Some(h) = own.nth(copy) {
            return Ok(Hardware {
                body: sheet.body.clone(),
                kind: h.kind,
                thread: h.thread.clone(),
                length: h.length,
                count: holes.iter().filter(|o| o.kind == h.kind && o.thread == h.thread && o.length == h.length).count(),
            });
        }
    }
    Err(if any { format!("the feature has no hardware hole {copy} now") } else { "the feature has no press-in hardware hole now".into() })
}

/// The text of `h`, one line: the count when more than one, the kind in capitals, the thread, and a
/// stud's or standoff's length after a `×`. A length in whole millimetres has no decimals, as a size
/// in a catalogue is written; any other length has the style's.
pub fn text(h: &Hardware, style: &TextStyle) -> Result<Vec<String>> {
    style.validate()?;
    let count = if h.count > 1 { format!("{}× ", h.count) } else { String::new() };
    let length = match h.length.filter(|_| h.kind.has_length()) {
        Some(mm) => {
            let whole = style.units == "mm" && (mm - mm.round()).abs() < 1e-9;
            let decimals = if whole { 0 } else { style.decimals };
            format!(" × {}", display_units::length_number(mm, &style.units, decimals))
        }
        None => String::new(),
    };
    Ok(vec![format!("{count}{} {}{length}", h.kind.label().to_uppercase(), h.thread)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> TextStyle {
        TextStyle { units: "mm".into(), decimals: 2, language: "en".into() }
    }

    fn hardware(kind: HardwareKind, length: Option<f64>, count: usize) -> Hardware {
        Hardware { body: "Plate".into(), kind, thread: "M4".into(), length, count }
    }

    #[test]
    fn text_has_the_count_the_kind_the_thread_and_a_length() {
        let nuts = Hardware { thread: "M5".into(), ..hardware(HardwareKind::FlushNut, None, 4) };
        assert_eq!(text(&nuts, &style()).unwrap(), ["4× FLUSH NUT M5"]);
        assert_eq!(text(&hardware(HardwareKind::Stud, Some(10.0), 2), &style()).unwrap(), ["2× STUD M4 × 10"]);
        assert_eq!(text(&hardware(HardwareKind::Standoff, Some(6.35), 1), &style()).unwrap(), ["STANDOFF M4 × 6.35"]);
        let bad = TextStyle { units: "furlong".into(), ..style() };
        assert!(text(&nuts, &bad).is_err());
    }

    #[test]
    fn a_copy_past_the_limit_is_refused() {
        assert!(HardwareRef { feature: 3, copy: MAX_COPIES }.validate().is_err());
        assert!(HardwareRef { feature: 3, copy: 0 }.validate().is_ok());
    }

    #[test]
    fn no_sheets_means_lost() {
        assert!(read(&[], &HardwareRef { feature: 3, copy: 0 }).is_err());
    }
}
