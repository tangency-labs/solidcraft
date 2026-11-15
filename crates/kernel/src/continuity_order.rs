//! The orders of geometric continuity, G0 to G4, in one table: the word a command parameter and
//! a design file use (`g0` … `g4`), the code shown in hover texts, help and messages (`G0` …
//! `G4`), and the message ids of the plain word a control shows (Position, Tangent, Curvature,
//! Curvature change, Smooth curvature change) and of its hover text. Labels never show the code.
//!
//! Each order adds one condition across a seam: G0 the position, G1 the tangent plane, G2 the
//! curvature, G3 how the curvature changes across the seam (its first derivative by arc length),
//! G4 how that change itself changes (the second derivative).
//!
//! [`Construction`] says which orders each construction builds today, and
//! [`Continuity::built_by`] is the one check that refuses the rest.

use crate::{KernelError, Result};

/// How smoothly a surface or curve continues across a seam: the number of derivatives it shares.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Continuity {
    /// The position alone: the surfaces meet, with a crease where they do.
    G0,
    /// Position and tangent plane.
    #[default]
    G1,
    /// Position, tangent plane and curvature.
    G2,
    /// Position, tangent plane, curvature and the change of curvature across the seam.
    G3,
    /// All of G3, and how that change of curvature itself changes.
    G4,
}

/// One row of the table: what an order is called where.
struct Words {
    /// The command parameter and design-file word.
    word: &'static str,
    /// The code, for hover texts, help and messages only.
    code: &'static str,
    /// The message id of the plain word a control shows.
    label: &'static str,
    /// The message id of the hover text, which names the code.
    tip: &'static str,
}

/// The table, indexed by [`Continuity::order`].
const WORDS: [Words; 5] = [
    Words { word: "g0", code: "G0", label: "ui.continuity_position", tip: "ui.continuity_position_tip" },
    Words { word: "g1", code: "G1", label: "ui.continuity_tangent", tip: "ui.continuity_tangent_tip" },
    Words { word: "g2", code: "G2", label: "ui.continuity_curvature", tip: "ui.continuity_curvature_tip" },
    Words { word: "g3", code: "G3", label: "ui.continuity_curvature_change", tip: "ui.continuity_curvature_change_tip" },
    Words { word: "g4", code: "G4", label: "ui.continuity_smooth_curvature_change", tip: "ui.continuity_smooth_curvature_change_tip" },
];

impl Continuity {
    /// Every order, from G0 up.
    pub const ALL: [Continuity; 5] = [Continuity::G0, Continuity::G1, Continuity::G2, Continuity::G3, Continuity::G4];

    /// The number of derivatives across the seam it fixes: 0 (G0) up to 4 (G4).
    pub fn order(self) -> usize {
        self as usize
    }

    /// The order with `order` derivatives, `None` past G4.
    pub fn from_order(order: usize) -> Option<Continuity> {
        Continuity::ALL.get(order).copied()
    }

    /// Conditions a side carries: the position and one per derivative, 1 (G0) up to 5 (G4).
    pub fn conditions(self) -> usize {
        self.order() + 1
    }

    /// The word a command parameter and a design use: `g0` … `g4`.
    pub fn as_str(self) -> &'static str {
        self.row().word
    }

    /// The code `G0` … `G4`: for hover texts, help and messages, never a label.
    pub fn code(self) -> &'static str {
        self.row().code
    }

    /// The message id of the plain word a control shows for this order.
    pub fn label_id(self) -> &'static str {
        self.row().label
    }

    /// The message id of this order's hover text (which names its code).
    pub fn tip_id(self) -> &'static str {
        self.row().tip
    }

    /// Parse a word (`g0` … `g4`, either case, surrounding blanks ignored); `None` for anything else.
    pub fn parse(s: &str) -> Option<Continuity> {
        let s = s.trim();
        Continuity::ALL.into_iter().find(|g| g.as_str().eq_ignore_ascii_case(s))
    }

    /// The words of `from` up to `to` for a message: "g0, g1 or g2".
    pub fn words(from: Continuity, to: Continuity) -> String {
        let words: Vec<&str> = Continuity::ALL.into_iter().filter(|g| (from..=to).contains(g)).map(Continuity::as_str).collect();
        match words.split_last() {
            Some((last, [])) => (*last).to_string(),
            Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
            None => String::new(),
        }
    }

    /// This order if `by` builds it, else a refusal that names the orders it does build. The one
    /// check every command and kernel entry point makes ([`Construction`]).
    pub fn built_by(self, by: Construction) -> Result<Continuity> {
        let (lowest, highest) = by.orders();
        if self < lowest {
            return Err(KernelError::Invalid(format!("{} takes {}", by.what(), Continuity::words(lowest, highest))));
        }
        if self > highest {
            return Err(KernelError::Invalid(format!(
                "not supported yet: {} is built with {}, not {}",
                by.what(),
                Continuity::words(lowest, highest),
                self.as_str()
            )));
        }
        Ok(self)
    }

    fn row(self) -> &'static Words {
        // `WORDS` has a row for every variant (`order` is at most 4).
        &WORDS[self.order()]
    }
}

/// What builds geometry to a continuity order, and the orders each one builds today.
///
/// HAND-OFF POINT: this table is the only place that says how far a construction goes. When a
/// construction learns a higher order, raise its `highest` in [`Construction::orders`]; every
/// command, dialog and kernel entry point reads it from here, so nothing else changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Construction {
    /// A patch filling a loop of edges (`surface.patch`).
    Patch,
    /// A bridge of Hermite rows: a blend surface, a boundary patch on a loop of sketch curves, a
    /// loft's ends and a match surface's seam.
    Bridge,
    /// A surface through a curve network (`surface.boundary` with `u` and `v` curves).
    Network,
    /// A match surface: a bridge that must at least share the tangent plane.
    Match,
    /// A sketch blend curve (`sketch.blend_curve`): one Bézier through G1 … G4.
    BlendCurve,
}

impl Construction {
    /// The lowest and the highest order this construction builds.
    pub fn orders(self) -> (Continuity, Continuity) {
        match self {
            Construction::Patch => (Continuity::G0, Continuity::G4),
            Construction::Bridge => (Continuity::G0, Continuity::G3),
            Construction::Network => (Continuity::G0, Continuity::G2),
            Construction::Match => (Continuity::G1, Construction::Bridge.orders().1),
            Construction::BlendCurve => (Continuity::G1, Continuity::G4),
        }
    }

    /// Whether this construction builds `g`.
    pub fn builds(self, g: Continuity) -> bool {
        let (lowest, highest) = self.orders();
        (lowest..=highest).contains(&g)
    }

    /// The orders it builds, from its lowest up: what a dialog offers.
    pub fn offered(self) -> Vec<Continuity> {
        Continuity::ALL.into_iter().filter(|g| self.builds(*g)).collect()
    }

    /// What it is called in a message.
    fn what(self) -> &'static str {
        match self {
            Construction::Patch => "a patch",
            Construction::Bridge => "a blend, boundary, loft or match surface",
            Construction::Network => "a curve network",
            Construction::Match => "a match surface",
            Construction::BlendCurve => "a blend curve",
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn every_order_round_trips_through_its_word_and_serde() {
        for (k, g) in Continuity::ALL.into_iter().enumerate() {
            assert_eq!(g.order(), k);
            assert_eq!(Continuity::from_order(k), Some(g));
            assert_eq!(g.conditions(), k + 1);
            assert_eq!(g.as_str(), format!("g{k}"));
            assert_eq!(g.code(), format!("G{k}"));
            assert_eq!(Continuity::parse(g.as_str()), Some(g));
            assert_eq!(Continuity::parse(&format!(" {} ", g.code())), Some(g));
            // The design file's word is the command word.
            assert_eq!(serde_json::to_value(g).unwrap(), serde_json::json!(g.as_str()));
            assert_eq!(serde_json::from_value::<Continuity>(serde_json::json!(g.as_str())).unwrap(), g);
        }
        assert_eq!(Continuity::from_order(5), None);
        for word in ["", "g5", "G-1", "curvature", "g 1"] {
            assert_eq!(Continuity::parse(word), None, "{word:?}");
        }
    }

    #[test]
    fn labels_are_words_and_tips_are_their_own() {
        let ids: Vec<&str> = Continuity::ALL.iter().flat_map(|g| [g.label_id(), g.tip_id()]).collect();
        for (i, id) in ids.iter().enumerate() {
            assert!(id.starts_with("ui.continuity_"), "{id}");
            assert!(!ids[..i].contains(id), "{id} twice");
            assert!(!id.contains("_g"), "a message id names its word, not its code: {id}");
        }
    }

    #[test]
    fn a_construction_refuses_what_it_does_not_build_and_names_what_it_does() {
        assert_eq!(Continuity::words(Continuity::G0, Continuity::G2), "g0, g1 or g2");
        assert_eq!(Continuity::words(Continuity::G4, Continuity::G4), "g4");
        assert_eq!(Continuity::G4.built_by(Construction::Patch).unwrap(), Continuity::G4);
        let e = Continuity::G3.built_by(Construction::Network).unwrap_err().to_string();
        assert!(e.contains("not supported yet") && e.contains("g0, g1 or g2") && e.contains("g3"), "{e}");
        let e = Continuity::G0.built_by(Construction::Match).unwrap_err().to_string();
        assert!(e.contains("g1, g2 or g3"), "{e}");
        assert!(Continuity::G4.built_by(Construction::Bridge).is_err());
        assert!(Continuity::G4.built_by(Construction::BlendCurve).is_ok());
        assert_eq!(Construction::Patch.offered(), Continuity::ALL.to_vec());
        assert_eq!(Construction::Network.offered(), vec![Continuity::G0, Continuity::G1, Continuity::G2]);
        assert_eq!(Construction::BlendCurve.offered().first(), Some(&Continuity::G1));
    }
}
