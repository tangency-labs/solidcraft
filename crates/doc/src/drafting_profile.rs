//! Drafting profiles: how a drawing writes its numbers (W09d, #4896). A drawing keeps a whole
//! copy of its profile, never only a name, so a later change to a built-in can't change how an
//! old drawing reads; `differs_from_library` reports such a change and never applies it.

use serde::{Deserialize, Serialize};

use crate::display_units::{DecimalMarker, DimensionStyle, MAX_DECIMALS, NumberFormat};
use crate::expr::Kind;
use crate::tolerance::Repeat;
use crate::{DocError, Result};

/// The longest id, and the longest name or edition, in characters.
const MAX_ID: usize = 64;
const MAX_TEXT: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftingProfile {
    /// Lower-case letters, digits and `_`, starting with a letter: a built-in's id, or another
    /// for a profile of the drawing's own.
    pub id: String,
    /// The built-in's revision this copy was taken from. A built-in whose rules change gets the
    /// next one.
    pub version: u32,
    /// The name people see, such as "ISO 129-1".
    pub name: String,
    /// The standard and edition it follows, or empty for a house profile.
    pub edition: String,
    pub number: NumberFormat,
    /// The decimals a new length, angle or tolerance gets (a dimension may set its own).
    pub length_decimals: u8,
    pub angle_decimals: u8,
    pub tolerance_decimals: u8,
    pub repeat: Repeat,
}

impl Default for DraftingProfile {
    /// The default templates' profile: ISO's rules with a decimal point.
    fn default() -> Self {
        DraftingProfile::new("iso_decimal_point", "ISO, decimal point", "ISO 129-1:2018", NumberFormat::default(), Repeat::Times)
    }
}

impl DraftingProfile {
    fn new(id: &str, name: &str, edition: &str, number: NumberFormat, repeat: Repeat) -> DraftingProfile {
        DraftingProfile {
            id: id.into(),
            version: 1,
            name: name.into(),
            edition: edition.into(),
            number,
            length_decimals: 2,
            angle_decimals: 2,
            tolerance_decimals: 2,
            repeat,
        }
    }

    /// The built-in profiles (the first is the default). ISO 129-1 4.1.1 gives the decimal
    /// comma. ASME's zero rules (millimetres: a leading zero, no trailing zeros) and `4X` are
    /// general knowledge, not read from the standard: data, no claim of conformance.
    pub fn library() -> Vec<DraftingProfile> {
        let point = NumberFormat::default();
        vec![
            DraftingProfile::default(),
            DraftingProfile::new("iso_129_1", "ISO 129-1", "ISO 129-1:2018", NumberFormat { marker: DecimalMarker::Comma, ..point }, Repeat::Times),
            DraftingProfile::new("asme_y14_5", "ASME Y14.5", "ASME Y14.5-2018", NumberFormat { trailing_zeros: false, ..point }, Repeat::X),
        ]
    }

    /// A profile read from a file or a command: what each field may hold.
    pub fn validate(&self) -> Result<()> {
        let id = self.id.as_bytes();
        let id_ok = id.len() <= MAX_ID
            && id.first().is_some_and(u8::is_ascii_lowercase)
            && id.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_');
        if !id_ok {
            return Err(invalid("a drafting profile's id is 1 to 64 lower-case letters, digits and `_`, starting with a letter"));
        }
        if self.version == 0 {
            return Err(invalid("a drafting profile's version starts at 1"));
        }
        let text_ok = |t: &str| t.chars().count() <= MAX_TEXT && !t.chars().any(char::is_control);
        if self.name.trim().is_empty() || !text_ok(&self.name) || !text_ok(&self.edition) {
            return Err(invalid("a drafting profile's name is 1 to 128 characters and its edition at most 128, without control characters"));
        }
        if [self.length_decimals, self.angle_decimals, self.tolerance_decimals].iter().any(|d| *d > MAX_DECIMALS) {
            return Err(invalid(&format!("a drafting profile gives at most {MAX_DECIMALS} decimals")));
        }
        Ok(())
    }

    /// What one dimension of `kind`, shown in `unit`, takes from the profile.
    pub fn style(&self, kind: Kind, unit: &str) -> Result<DimensionStyle> {
        let decimals = match kind {
            Kind::Length => self.length_decimals,
            Kind::Angle => self.angle_decimals,
            Kind::Unitless => return Err(invalid("a dimension is a length or an angle")),
        };
        Ok(DimensionStyle { unit: unit.into(), number: self.number, decimals, tolerance_decimals: self.tolerance_decimals, repeat: self.repeat })
    }

    /// The built-in this copy came from, when it isn't the same any more: this copy's version
    /// and the library's. Reported (the drawing check), never applied.
    pub fn differs_from_library(&self) -> Option<(u32, u32)> {
        let current = DraftingProfile::library().into_iter().find(|p| p.id == self.id)?;
        (current != *self).then_some((self.version, current.version))
    }
}

fn invalid(message: &str) -> DocError {
    DocError::Invalid(message.into())
}
