//! Single-row geometrical tolerance indicators and datum identifiers, below the UI.
//!
//! These are bounded semantic records, not model associations or manufacturing certification.
//! [`Frame::validate`] checks the supported grammar; [`Frame::warnings`] reports context that
//! only a future drawing checker can resolve. Text and geometry entry points validate again,
//! including records obtained through serde. Encoded-input callers must also bound input bytes.
//!
//! Geometry includes explicitly provisional proportions, isolated for later standards correction.
//! It must not be treated as standards-compliant manufacturing annotation.
//! The supported editions, rules, original symbol construction and evidence limits are recorded
//! in `docs/drawing-gdt.md`. No sheet placement, leaders, targets or composite frames live here.

use serde::{Deserialize, Serialize};

mod geometry;
pub use geometry::{Layout, Style};

/// The complete edition choice for this subset. ISO uses ISO 1101:2017 and ISO 5459:2024;
/// ASME uses Y14.5-2018 (reaffirmed 2024). Neither is an alias for the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standard {
    Iso1101_2017Iso5459_2024,
    AsmeY14_5_2018,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Characteristic {
    Straightness,
    Flatness,
    Roundness,
    Cylindricity,
    LineProfile,
    SurfaceProfile,
    Parallelism,
    Perpendicularity,
    Angularity,
    Position,
    /// ISO only in the supported editions; includes the coaxiality indication.
    Concentricity,
    /// ISO only in the supported editions.
    Symmetry,
    CircularRunout,
    TotalRunout,
}

impl Characteristic {
    pub const ALL: [Self; 14] = {
        use Characteristic::*;
        [
            Straightness,
            Flatness,
            Roundness,
            Cylindricity,
            LineProfile,
            SurfaceProfile,
            Parallelism,
            Perpendicularity,
            Angularity,
            Position,
            Concentricity,
            Symmetry,
            CircularRunout,
            TotalRunout,
        ]
    };

    /// A text transcription, never used as a font glyph to draw the symbol.
    pub fn text(self) -> &'static str {
        match self {
            Self::Straightness => "⏤",
            Self::Flatness => "⏥",
            Self::Roundness => "○",
            Self::Cylindricity => "⌭",
            Self::LineProfile => "⌒",
            Self::SurfaceProfile => "⌓",
            Self::Parallelism => "∥",
            Self::Perpendicularity => "⟂",
            Self::Angularity => "∠",
            Self::Position => "⌖",
            Self::Concentricity => "◎",
            Self::Symmetry => "⌯",
            Self::CircularRunout => "↗",
            Self::TotalRunout => "⌰",
        }
    }

    fn is_form(self) -> bool {
        matches!(self, Self::Straightness | Self::Flatness | Self::Roundness | Self::Cylindricity)
    }

    fn requires_datums(self) -> bool {
        matches!(
            self,
            Self::Parallelism
                | Self::Perpendicularity
                | Self::Angularity
                | Self::Concentricity
                | Self::Symmetry
                | Self::CircularRunout
                | Self::TotalRunout
        )
    }
}

/// A semantic distinction needed to validate material modifiers and zone shapes. It does not
/// attach anything to a face, edge or dimension. The caller must eventually verify it on a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    Surface,
    DerivedFeatureOfSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneShape {
    Width,
    Diameter,
    SphericalDiameter,
}

/// ISO: maximum/least material requirement (ISO 2692 governs the associated evaluation).
/// ASME: maximum/least material condition on the tolerance, maximum/least material boundary
/// on a datum reference. Absence means the family's default; no obsolete circled S is emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialModifier {
    Maximum,
    Least,
}

impl MaterialModifier {
    fn text(self) -> &'static str {
        match self {
            Self::Maximum => "Ⓜ",
            Self::Least => "Ⓛ",
        }
    }
}

/// One tolerance compartment. Values are millimetres, independent of view scale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    pub value_mm: f64,
    pub shape: ZoneShape,
    pub material: Option<MaterialModifier>,
    /// Supported only for ASME position, where the positive height is inside the frame.
    /// ISO's externally dimensioned projected zone is deliberately not represented this way.
    pub projected_height_mm: Option<f64>,
}

/// One single-datum compartment; its vector order in [`Frame`] is primary to tertiary.
/// Common datums and their component-specific modifiers are outside this first subset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatumReference {
    pub identifier: String,
    pub material: Option<MaterialModifier>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub standard: Standard,
    pub characteristic: Characteristic,
    pub feature: Feature,
    pub zone: Zone,
    pub datums: Vec<DatumReference>,
}

/// A boxed datum label, without a triangle/leader or model attachment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatumIdentifier {
    pub standard: Standard,
    pub letters: String,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("geometrical tolerance: {0}")]
pub struct Invalid(pub &'static str);

pub type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

fn identifier(letters: &str) -> Result<()> {
    require(
        (1..=2).contains(&letters.len()) && letters.bytes().all(|b| b.is_ascii_uppercase() && !matches!(b, b'I' | b'O' | b'Q')),
        "a datum identifier is one or two capital ASCII letters, excluding I, O and Q",
    )
}

// Resource limits, not standards limits. The lower positive bound also bounds text length.
fn positive_length(value: f64) -> bool {
    (1e-9..=10_000.0).contains(&value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    /// Legal syntax for a pattern; the drawing checker must establish the pattern context.
    DatumFreePositionNeedsPatternContext,
}

impl Frame {
    pub fn validate(&self) -> Result<()> {
        use Characteristic::*;
        require(self.datums.len() <= 3, "at most three ordered datum compartments are supported")?;
        require(
            self.standard != Standard::AsmeY14_5_2018 || !matches!(self.characteristic, Concentricity | Symmetry),
            "concentricity and symmetry are not supported by ASME Y14.5-2018",
        )?;
        require(!self.characteristic.is_form() || self.datums.is_empty(), "form tolerances have no datum references")?;
        require(!self.characteristic.requires_datums() || !self.datums.is_empty(), "this characteristic requires a datum reference")?;
        for (i, datum) in self.datums.iter().enumerate() {
            identifier(&datum.identifier)?;
            require(!self.datums.iter().take(i).any(|d| d.identifier == datum.identifier), "a datum cannot recur in an ordered datum system")?;
        }
        let derived = self.feature == Feature::DerivedFeatureOfSize;
        let derived_allowed = matches!(
            self.characteristic,
            Straightness | Flatness | Parallelism | Perpendicularity | Angularity | Position | Concentricity | Symmetry
        );
        require(!derived || derived_allowed, "this characteristic controls a surface in the supported subset")?;
        require(
            !matches!(self.characteristic, Position | Concentricity | Symmetry) || derived,
            "this characteristic requires a derived feature of size in the supported subset",
        )?;
        let diameter_allowed =
            derived && matches!(self.characteristic, Straightness | Parallelism | Perpendicularity | Angularity | Position | Concentricity);
        require(self.zone.shape == ZoneShape::Width || diameter_allowed, "a diameter zone is not supported for this feature and characteristic")?;
        require(
            self.zone.shape != ZoneShape::SphericalDiameter || self.characteristic == Position,
            "a spherical zone is supported only for position",
        )?;
        require(
            self.characteristic != Concentricity || self.zone.shape == ZoneShape::Diameter,
            "the supported ISO concentricity/coaxiality indication uses a diameter zone",
        )?;
        require(
            self.zone.material.is_none()
                || (derived && matches!(self.characteristic, Straightness | Flatness | Parallelism | Perpendicularity | Angularity | Position)),
            "a material modifier is supported only for a size feature's straightness, flatness, orientation or position",
        )?;
        require(
            positive_length(self.zone.value_mm) || (self.zone.value_mm == 0.0 && self.zone.material == Some(MaterialModifier::Maximum)),
            "tolerance is finite, 1e-9 to 10000 mm, or zero at maximum material",
        )?;
        if let Some(height) = self.zone.projected_height_mm {
            require(
                self.standard == Standard::AsmeY14_5_2018 && self.characteristic == Position,
                "an in-frame projected height is supported only for ASME position",
            )?;
            require(self.zone.shape != ZoneShape::SphericalDiameter, "a spherical zone cannot be projected")?;
            require(positive_length(height), "projected height is finite and 1e-9 to 10000 mm")?;
        }
        Ok(())
    }

    pub fn warnings(&self) -> Result<Vec<Warning>> {
        self.validate()?;
        Ok(if self.characteristic == Characteristic::Position && self.datums.is_empty() {
            vec![Warning::DatumFreePositionNeedsPatternContext]
        } else {
            Vec::new()
        })
    }

    /// Compartments in reading order. This is a transcription, not a parser or interchange format.
    pub fn compartments(&self) -> Result<Vec<String>> {
        self.validate()?;
        let mut cells = vec![self.characteristic.text().into(), self.zone.text()];
        cells.extend(self.datums.iter().map(|d| {
            let mut text = d.identifier.clone();
            if let Some(material) = d.material {
                text.push_str(material.text());
            }
            text
        }));
        Ok(cells)
    }

    /// `⌖ | ⌀0.1Ⓜ | A | B | C`, for example. Decimal point and millimetres are fixed here;
    /// locale, unit conversion and display precision belong to the later annotation layer.
    pub fn text(&self) -> Result<String> {
        Ok(self.compartments()?.join(" | "))
    }

    pub fn layout(&self, style: Style) -> Result<Layout> {
        geometry::frame(self, style)
    }
}

impl Zone {
    fn text(&self) -> String {
        let mut text = match self.shape {
            ZoneShape::Width => String::new(),
            ZoneShape::Diameter => "⌀".into(),
            ZoneShape::SphericalDiameter => "S⌀".into(),
        };
        // Normalise negative zero; no accepted finite value is rounded away.
        text.push_str(&if self.value_mm == 0.0 { "0".into() } else { self.value_mm.to_string() });
        if let Some(material) = self.material {
            text.push_str(material.text());
        }
        if let Some(height) = self.projected_height_mm {
            text.push_str(&format!("Ⓟ{height}"));
        }
        text
    }
}

impl DatumIdentifier {
    pub fn validate(&self) -> Result<()> {
        identifier(&self.letters)
    }

    pub fn text(&self) -> Result<String> {
        self.validate()?;
        Ok(format!("[{}]", self.letters))
    }

    pub fn layout(&self, style: Style) -> Result<Layout> {
        geometry::datum(self, style)
    }
}

#[cfg(test)]
#[path = "gdt_tests.rs"]
mod tests;
