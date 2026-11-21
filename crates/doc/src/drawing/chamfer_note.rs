//! The content of a chamfer note (#13028): a chamfer's size, worked out from the model's Chamfer
//! feature, and its text. It follows the hole callouts' pattern ([`super::hole_callout`]).
//!
//! - A note names one chamfer by persistent ids only ([`ChamferRef`]): the Chamfer feature's id and
//!   the persistent name of an edge it cut, as the feature keeps it (`Feature::edge_names`). Never a
//!   face index, an edge index or a nearest point. The chamfer's face on that edge is named after
//!   it (`F<id>:blend(<edge>)`, or the older `F<id>:blend:<edge>`), and keeps that name in the pieces
//!   a later cut splits it into, so the name finds the chamfer again. Along a tangent chain the
//!   other faces are named after their own edges (or, without exact history, the picked edge
//!   nearest them); the note looks only for the picked edge's own face, which both namings make.
//! - It resolves to one of four outcomes ([`Resolution`]), with the ids read as a hole callout
//!   reads them ([`super::feature_note::feature`]): resolved; suppressed (the feature is); failed
//!   (the feature did not evaluate, or no face of this chamfer is in the model as it is now: a
//!   later feature removed it); lost (the feature is gone). A lost note keeps its ids and is never
//!   moved to another chamfer. An edge the feature doesn't cut is refused.
//! - The size comes from the feature record, evaluated with the design's parameters as the model
//!   evaluates it, so the note follows edits. The record's `distance` runs along the first face
//!   (the one facing up most, the other with `flip`). The note lists the setback on the face facing
//!   up most first, so `flip` swaps two distances (`2 × 3`, `3 × 2`).
//! - The text: lengths through the shared length formatter ([`crate::display_units`]), angles as
//!   the hole callouts write them ([`TextStyle::degrees`]). Nothing is read from a view, and
//!   nothing is saved.
//!
//! Chamfers made by other features (a cut extrude, an imported body), a Chamfer saved before its
//! edges had names, and the copies a pattern or a mirror makes are not supported yet.

use serde::{Deserialize, Serialize};

use super::feature_note::{self, TextStyle};
use crate::expr::Kind;
use crate::persistent_name::{self, Base, EdgeName, FaceName, FeatureId, Rounds, Suffix};
use crate::{DocError, Document, FeatureKind, Model, Result, display_units};

/// The most bytes of an edge name a note takes.
pub const MAX_EDGE_BYTES: usize = 2048;
/// The most edges one Chamfer feature cuts (as the kernel takes them).
const MAX_EDGES: usize = 1000;
/// The largest length a note shows, in millimetres.
const MAX_LENGTH_MM: f64 = 1e9;

/// One chamfer, by persistent ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChamferRef {
    /// The Chamfer feature's id.
    pub feature: u64,
    /// The persistent name of an edge it cuts.
    pub edge: String,
}

/// What a note's reference comes to.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Resolution {
    Resolved {
        note: ChamferNote,
    },
    /// The feature is suppressed.
    Suppressed,
    /// The feature did not evaluate, or a later feature removed the chamfer.
    Failed {
        reason: String,
    },
    /// The feature is gone.
    Lost {
        reason: String,
    },
}

/// A resolved chamfer: its size, in millimetres and radians.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChamferNote {
    /// How many edges the feature cuts (its picked edges: a tangent chain is one).
    pub count: usize,
    pub size: Size,
}

/// A chamfer's size.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Size {
    /// The same setback on both faces, at 45°.
    Equal { distance: f64 },
    /// The setback along the first face, and the angle the chamfer makes with it.
    Angle { distance: f64, angle: f64 },
    /// The setback along the face facing up most, then along the other.
    Distances { up: f64, other: f64 },
}

impl ChamferRef {
    /// Refuses an edge name that is empty, too long or holds control characters.
    pub fn validate(&self) -> Result<()> {
        let e = &self.edge;
        if e.is_empty() || e.len() > MAX_EDGE_BYTES || e.chars().any(char::is_control) {
            return Err(DocError::Invalid(format!("an edge name is 1 to {MAX_EDGE_BYTES} bytes, without control characters")));
        }
        Ok(())
    }
}

/// The note of the chamfer `chamfer` names, in the design `doc` evaluated as `model`.
pub fn resolve(doc: &Document, model: &Model, chamfer: &ChamferRef) -> Result<Resolution> {
    chamfer.validate()?;
    let f = match feature_note::feature(doc, chamfer.feature, "Chamfer")? {
        Ok(f) => f,
        Err(reason) => return Ok(Resolution::Lost { reason }),
    };
    let FeatureKind::Chamfer { distance, distance2, angle, flip, .. } = &f.kind else {
        return Err(DocError::Invalid(format!(
            "not supported yet: chamfer notes of {} features (only Chamfer features have them)",
            f.kind.base_name()
        )));
    };
    if f.edge_names.is_empty() {
        return Err(DocError::Invalid("not supported yet: a chamfer saved before its edges had names (edit it once to name them)".into()));
    }
    if f.edge_names.len() > MAX_EDGES {
        return Err(DocError::Invalid(format!("a chamfer cuts 1…{MAX_EDGES} edges")));
    }
    if !f.edge_names.contains(&chamfer.edge) {
        return Err(DocError::Invalid(format!("feature {} doesn't cut that edge", f.id)));
    }
    if f.suppressed {
        return Ok(Resolution::Suppressed);
    }
    match model.result(f.id) {
        None => return Ok(Resolution::Failed { reason: "not evaluated".into() }),
        Some(r) if r.skipped => return Ok(Resolution::Failed { reason: "not evaluated: it is after the end of the timeline".into() }),
        Some(r) if r.error.is_some() => return Ok(Resolution::Failed { reason: r.error.clone().unwrap_or_default() }),
        Some(_) => {}
    }
    // Its face is named after the edge, in the model as it is now: none left, a later feature removed it.
    // An edge name that doesn't read names no face.
    let here = EdgeName::parse(&chamfer.edge).is_ok_and(|edge| {
        let state = model.state();
        state.bodies.iter().any(|b| crate::naming::face_names(b).iter().any(|n| rounds(n, f.id, &edge)))
    });
    if !here {
        return Ok(Resolution::Failed { reason: "a later feature removed the chamfer".into() });
    }
    let (vals, _) = crate::eval::evaluation_values(doc);
    let length = |e: &str, what: &str| -> Result<f64> {
        let v = crate::eval::val(&vals, e, Kind::Length)?;
        if !(v.is_finite() && v > 0.0 && v < MAX_LENGTH_MM) {
            return Err(DocError::Invalid(format!("the chamfer's {what} is not a positive length")));
        }
        Ok(v)
    };
    let d = length(distance, "distance")?;
    let size = match (distance2, angle) {
        (Some(e), _) => {
            let d2 = length(e, "second distance")?;
            // The record's `distance` runs along the first face; with `flip` that is the other one.
            let (up, other) = if *flip { (d2, d) } else { (d, d2) };
            Size::Distances { up, other }
        }
        (None, Some(e)) => {
            let a = crate::eval::val(&vals, e, Kind::Angle)?;
            if !(a.is_finite() && a > 0.0 && a < std::f64::consts::PI) {
                return Err(DocError::Invalid("the chamfer's angle is not between 0 and 180°".into()));
            }
            Size::Angle { distance: d, angle: a }
        }
        (None, None) => Size::Equal { distance: d },
    };
    Ok(Resolution::Resolved { note: ChamferNote { count: f.edge_names.len(), size } })
}

/// The note's text: `d × 45°`, `d × a` or `up × other`, with `n × (…)` in front when `count` is
/// asked for and the feature cuts more than one edge.
pub fn text(note: &ChamferNote, style: &TextStyle, count: bool) -> Result<String> {
    style.validate()?;
    let len = |mm: f64| display_units::length_number(mm, &style.units, style.decimals);
    let size = match note.size {
        Size::Equal { distance } => format!("{} × {}", len(distance), style.degrees(std::f64::consts::FRAC_PI_4)),
        Size::Angle { distance, angle } => format!("{} × {}", len(distance), style.degrees(angle)),
        Size::Distances { up, other } => format!("{} × {}", len(up), len(other)),
    };
    Ok(if count && note.count > 1 { format!("{} × ({size})", note.count) } else { size })
}

/// Whether face `name` is the face the Chamfer `feature` made on `edge`, read as a name, so either
/// text form: whole, or a piece of it a later cut made. Not a pattern's or a mirror's copy of it.
/// Only the names `feature` starts are parsed: about one per edge it cut.
fn rounds(name: &str, feature: u64, edge: &EdgeName) -> bool {
    persistent_name::made_by(name) == Some(feature)
        && FaceName::parse(name).is_ok_and(|face| {
            let own = matches!(&face.base, Base::Blend { feature: FeatureId(id), rounds: Rounds::Edge(e) } if *id == feature && **e == *edge);
            own && !face.suffixes.iter().any(|x| matches!(x, Suffix::Copy { .. }))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(units: &str, decimals: u8) -> TextStyle {
        TextStyle { units: units.into(), decimals, language: "en".into() }
    }

    fn note(count: usize, size: Size) -> ChamferNote {
        ChamferNote { count, size }
    }

    #[test]
    fn text_has_the_size_and_the_count_when_asked() {
        let equal = note(4, Size::Equal { distance: 2.0 });
        assert_eq!(text(&equal, &style("mm", 0), false).unwrap(), "2 × 45°");
        assert_eq!(text(&equal, &style("mm", 2), false).unwrap(), "2.00 × 45.00°");
        assert_eq!(text(&equal, &style("mm", 0), true).unwrap(), "4 × (2 × 45°)");
        // One edge: no count, asked for or not.
        assert_eq!(text(&note(1, Size::Equal { distance: 2.0 }), &style("mm", 0), true).unwrap(), "2 × 45°");
        let angled = note(1, Size::Angle { distance: 2.0, angle: 30f64.to_radians() });
        assert_eq!(text(&angled, &style("mm", 0), false).unwrap(), "2 × 30°");
        assert_eq!(text(&note(1, Size::Distances { up: 2.0, other: 3.0 }), &style("mm", 0), false).unwrap(), "2 × 3");
        // 2.54 mm is 0.1 in, shown with the inch minimum of 3 decimals.
        assert_eq!(text(&note(1, Size::Equal { distance: 2.54 }), &style("in", 2), false).unwrap(), "0.100 × 45.00°");
    }

    #[test]
    fn a_bad_style_or_edge_is_refused() {
        let c = note(1, Size::Equal { distance: 2.0 });
        for bad in [style("furlong", 1), style("mm", 7)] {
            assert!(text(&c, &bad, false).is_err(), "{bad:?}");
        }
        for edge in ["", "a\nb", &"e".repeat(MAX_EDGE_BYTES + 1)] {
            assert!(ChamferRef { feature: 1, edge: edge.into() }.validate().is_err(), "{edge:?}");
        }
        assert!(ChamferRef { feature: 1, edge: "F1:+x|F1:+z".into() }.validate().is_ok());
    }
}
