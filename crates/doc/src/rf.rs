//! Radio-frequency properties of materials, for Wi-Fi coverage analysis (`inspect.rf_coverage`).
//!
//! A material may say how much a Wi-Fi signal loses each time a straight path crosses a piece of
//! it: one loss in dB for each band (2.4, 5 and 6 GHz). The values are per crossing, not per
//! millimetre, the way multi-wall indoor models count walls. The presets are typical published
//! averages for building materials, rounded; real walls vary by several dB.

use serde::{Deserialize, Serialize};

/// The Wi-Fi bands the analysis knows, in the order [`RfLoss`] stores them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Band {
    /// 2.4 GHz.
    G2_4,
    /// 5 GHz.
    G5,
    /// 6 GHz.
    G6,
}

impl Band {
    pub const ALL: [Band; 3] = [Band::G2_4, Band::G5, Band::G6];

    /// The band's name in parameters and replies: `"2.4"`, `"5"` or `"6"`.
    pub fn name(self) -> &'static str {
        match self {
            Band::G2_4 => "2.4",
            Band::G5 => "5",
            Band::G6 => "6",
        }
    }

    /// The band its name gives (`"2.4"`, `"5"`, `"6"`, with or without a `GHz` suffix).
    pub fn parse(s: &str) -> Option<Band> {
        let s = s.trim();
        let s = s.strip_suffix("GHz").or_else(|| s.strip_suffix("ghz")).unwrap_or(s).trim();
        match s {
            "2.4" | "2" => Some(Band::G2_4),
            "5" => Some(Band::G5),
            "6" => Some(Band::G6),
            _ => None,
        }
    }

    /// The frequency the path loss is worked out at, in MHz: the middle of each band's channels.
    pub fn mhz(self) -> f64 {
        match self {
            Band::G2_4 => 2437.0,
            Band::G5 => 5500.0,
            Band::G6 => 6525.0,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Band::G2_4 => 0,
            Band::G5 => 1,
            Band::G6 => 2,
        }
    }
}

/// The largest loss one crossing may have, in dB: more than any building material, small enough
/// that sums stay finite.
pub const MAX_LOSS_DB: f64 = 100.0;

/// A material's loss each time a straight path crosses a piece of it, in dB, at 2.4, 5 and 6 GHz.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RfLoss(pub [f64; 3]);

impl RfLoss {
    /// The loss in `band`.
    pub fn at(&self, band: Band) -> f64 {
        self.0.get(band.index()).copied().unwrap_or(0.0)
    }

    /// Whether every loss is a number from 0 to [`MAX_LOSS_DB`].
    pub fn is_valid(&self) -> bool {
        self.0.iter().all(|l| l.is_finite() && (0.0..=MAX_LOSS_DB).contains(l))
    }
}

/// A file's `rf_loss_db`: losses out of range, or anything else unreadable, read as none, so a
/// damaged file opens and the analysis guesses those walls instead.
pub fn valid_loss<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<RfLoss>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value::<RfLoss>(value).ok().filter(RfLoss::is_valid))
}

/// A named set of losses `material.create` and `material.edit` take by name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RfPreset {
    /// The name parameters use, lower case with dashes.
    pub id: &'static str,
    pub loss: RfLoss,
}

/// The built-in RF presets.
pub const RF_PRESETS: &[RfPreset] = &[
    RfPreset { id: "concrete", loss: RfLoss([12.0, 20.0, 24.0]) },
    RfPreset { id: "drywall", loss: RfLoss([3.0, 4.0, 5.0]) },
    RfPreset { id: "interior-glass", loss: RfLoss([2.0, 3.0, 3.0]) },
    RfPreset { id: "low-e-glass", loss: RfLoss([10.0, 15.0, 18.0]) },
    RfPreset { id: "metal", loss: RfLoss([15.0, 20.0, 22.0]) },
    RfPreset { id: "none", loss: RfLoss([0.0, 0.0, 0.0]) },
    RfPreset { id: "wood-door", loss: RfLoss([3.0, 4.0, 5.0]) },
];

/// The preset `id` names (letter case, spaces and underscores don't matter).
pub fn rf_preset(id: &str) -> Option<&'static RfPreset> {
    let key: String = id.trim().chars().map(|c| if c == '_' || c == ' ' { '-' } else { c.to_ascii_lowercase() }).collect();
    RF_PRESETS.iter().find(|p| p.id == key)
}

/// The preset whose losses are exactly `loss`, if one is.
pub fn rf_preset_of(loss: &RfLoss) -> Option<&'static RfPreset> {
    RF_PRESETS.iter().find(|p| p.loss == *loss)
}

/// The losses of a built-in physical material ([`crate::MATERIALS`]): metals block like metal, oak
/// like a wood door, glass like interior glass. `None` for the others: the analysis then guesses
/// from the wall's thickness ([`heuristic_loss`]).
pub fn builtin_rf_loss(material: &str) -> Option<RfLoss> {
    let preset = match material {
        "Steel" | "Stainless Steel" | "Aluminum" | "Brass" | "Copper" | "Titanium" | "Cast Iron" => "metal",
        "Oak" => "wood-door",
        "Glass" => "interior-glass",
        _ => return None,
    };
    rf_preset(preset).map(|p| p.loss)
}

/// A wall at least this thick (mm) without an RF material counts as concrete, a thinner one as
/// drywall ([`heuristic_loss`]). Between a 4⅞″ steel-stud partition (124 mm) and a 6″ concrete
/// wall (152 mm).
pub const CONCRETE_FROM_MM: f64 = 140.0;

/// What a wall `thickness_mm` thick and without an RF material loses: drywall below
/// [`CONCRETE_FROM_MM`], concrete from there on.
pub fn heuristic_loss(thickness_mm: f64) -> (&'static str, RfLoss) {
    let id = if thickness_mm >= CONCRETE_FROM_MM { "concrete" } else { "drywall" };
    (id, rf_preset(id).map(|p| p.loss).unwrap_or(RfLoss([0.0; 3])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_sorted_valid_and_found_by_loose_names() {
        assert!(RF_PRESETS.windows(2).all(|w| w[0].id < w[1].id));
        assert!(RF_PRESETS.iter().all(|p| p.loss.is_valid()));
        assert_eq!(rf_preset("Low_E glass").map(|p| p.id), Some("low-e-glass"));
        assert_eq!(rf_preset("concrete").map(|p| p.loss.at(Band::G6)), Some(24.0));
        assert!(rf_preset("brick").is_none());
    }

    #[test]
    fn thickness_decides_drywall_or_concrete() {
        assert_eq!(heuristic_loss(92.0).0, "drywall");
        assert_eq!(heuristic_loss(139.9).0, "drywall");
        assert_eq!(heuristic_loss(140.0).0, "concrete");
        assert_eq!(heuristic_loss(203.0).1, RfLoss([12.0, 20.0, 24.0]));
    }

    #[test]
    fn bands_parse_their_names() {
        for b in Band::ALL {
            assert_eq!(Band::parse(b.name()), Some(b));
        }
        assert_eq!(Band::parse("5 GHz"), Some(Band::G5));
        assert_eq!(Band::parse("60"), None);
        assert!(!RfLoss([1.0, f64::NAN, 2.0]).is_valid());
        assert!(!RfLoss([1.0, -1.0, 2.0]).is_valid());
    }

    #[test]
    fn a_damaged_file_loss_reads_as_none() {
        #[derive(Deserialize)]
        struct M {
            #[serde(default, deserialize_with = "valid_loss")]
            rf: Option<RfLoss>,
        }
        let read = |j: &str| serde_json::from_str::<M>(j).map(|m| m.rf).ok();
        assert_eq!(read(r#"{"rf":[1,2,3]}"#), Some(Some(RfLoss([1.0, 2.0, 3.0]))));
        assert_eq!(read(r#"{"rf":[1,2,300]}"#), Some(None));
        assert_eq!(read(r#"{"rf":"x"}"#), Some(None));
        assert_eq!(read(r#"{}"#), Some(None));
    }
}
