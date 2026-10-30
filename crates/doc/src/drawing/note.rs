//! A drawing's notes (W19e, #4896): text on a sheet, in its fields and its words, that may point at
//! the model with one leader.
//!
//! - **The anchor** says what a note belongs to: a sheet, a view, or an edge or a corner of a view's
//!   model, saved by name as a dimension's is ([`super::dimension`]). It is one type so that other
//!   symbols placed on a drawing can use it too.
//! - **The place** is on paper: in millimetres from the sheet's lower-left corner for a note on a
//!   sheet, and from the view's centre for any other, so that note moves with its view.
//! - **The content** is the title block's text with fields ([`Content`]), so a sheet's number works
//!   in a note. It reads what a note can name: never an answer given when a block was placed, or a
//!   table's row.
//! - **The leader** is worked out, never saved, from where the anchor stands with the model now
//!   ([`Document::resolve_notes`]). A lost reference keeps the note and its text where they are,
//!   without a leader.
//!
//! The rules, beside the record's: its sheet or view is one of the drawing's; an edge or a corner
//! keeps a dimension's reference rules; its place is on paper ([`is_paper_coordinate`]); its text
//! height is [`CAP_MM`]; at most [`MAX_NOTES`] notes, listed by id.

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::cell::{Content, Part, Source};
use solidcraft_geom::drawing_view::is_paper_coordinate;
use solidcraft_geom::paper::MAX_SIDE_MM;

use super::by_id::Record;
use super::dimension::{Lookup, ModelEdge, ModelPoint, Status, check_edge, check_point};
use super::{Drawing, require};
use crate::{DocError, Document, Model, Result, ids};

/// The most notes a drawing may have.
pub const MAX_NOTES: usize = 10_000;
/// The text heights a note may set, in millimetres of paper.
pub const CAP_MM: RangeInclusive<f64> = 0.5..=100.0;

/// What a note belongs to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Anchor {
    /// A sheet, by its id.
    Sheet(#[serde(with = "crate::ids")] u64),
    /// A view, by its id.
    View(#[serde(with = "crate::ids")] u64),
    /// An edge of a view's model: its leader ends on the edge's middle.
    Edge {
        #[serde(with = "crate::ids")]
        view: u64,
        edge: ModelEdge,
    },
    /// A corner or a centre of a view's model: its leader ends there.
    Corner {
        #[serde(with = "crate::ids")]
        view: u64,
        point: ModelPoint,
    },
}

impl Anchor {
    /// The view it moves with, if any.
    pub fn view(&self) -> Option<u64> {
        match self {
            Anchor::Sheet(_) => None,
            Anchor::View(view) | Anchor::Edge { view, .. } | Anchor::Corner { view, .. } => Some(*view),
        }
    }

    /// The same anchor on view `view`, when it is on one.
    pub fn on_view(&self, view: u64) -> Anchor {
        let mut anchor = self.clone();
        if let Anchor::View(on) | Anchor::Edge { view: on, .. } | Anchor::Corner { view: on, .. } = &mut anchor {
            *on = view;
        }
        anchor
    }
}

/// One note of a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    #[serde(with = "crate::ids")]
    pub id: u64,
    pub anchor: Anchor,
    /// Where it sits, in millimetres of paper from its sheet's corner, or from its view's centre.
    pub at: [f64; 2],
    pub content: Content,
    /// The height of its capitals on paper, when it sets its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap_mm: Option<f64>,
}

impl Record for Note {
    fn id(&self) -> u64 {
        self.id
    }
}

impl Drawing {
    /// The notes' rules (the module's notes). The views are checked first, so they are in id order
    /// here.
    pub(super) fn check_notes(&self) -> Result<()> {
        self.notes.check(MAX_NOTES, "note", |n| self.check_note(n))
    }

    fn check_note(&self, n: &Note) -> Result<()> {
        match &n.anchor {
            Anchor::Sheet(sheet) => {
                require(self.sheets.iter().any(|s| s.id == *sheet), || {
                    format!("it is on sheet {}, which the drawing doesn't have", ids::encode(*sheet))
                })?;
            }
            Anchor::View(_) => {}
            Anchor::Edge { edge, .. } => check_edge(edge)?,
            Anchor::Corner { point, .. } => check_point(point)?,
        }
        if let Some(view) = n.anchor.view() {
            require(self.view(view).is_some(), || format!("it is on view {}, which the drawing doesn't have", ids::encode(view)))?;
        }
        require(!n.content.0.is_empty(), || "it has some text".into())?;
        n.content.validate().map_err(|e| DocError::Invalid(e.0.into()))?;
        let can_read = |part: &Part| !matches!(part, Part::Field { source: Source::Prompt | Source::Row, .. });
        require(n.content.0.iter().all(can_read), || "its fields read the drawing, its sheet, its component or its newest revision".into())?;
        let [x, y] = n.at;
        require(is_paper_coordinate(x) && is_paper_coordinate(y), || format!("its place is at most {MAX_SIDE_MM} mm on paper"))?;
        require(n.cap_mm.is_none_or(|cap| CAP_MM.contains(&cap)), || format!("its text height is {} to {} mm", CAP_MM.start(), CAP_MM.end()))
    }
}

/// A note as the model has it now ([`Document::resolve_notes`]): never saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    /// Always [`Status::Ok`] for a note on a sheet or a view, which reads no model.
    pub status: Status,
    /// Where its leader ends, in the design's frame: when it has one.
    pub point: Option<Vec3>,
    /// What it points at was found by exact history, as one edge.
    pub exact: bool,
}

impl Document {
    /// The notes `wanted` of `drawing` (one of this design's drawings) as `model` (this design's) has
    /// them now, as [`Document::resolve_balloons`] finds balloons'. `Err` only for an id the drawing
    /// doesn't have.
    pub fn resolve_notes(&self, model: &Model, drawing: &Drawing, wanted: &[u64]) -> Result<Vec<Resolved>> {
        let mut lookup = Lookup::new(self, model);
        wanted
            .iter()
            .map(|id| {
                let n = drawing.notes.by_id(*id).ok_or_else(|| DocError::Unknown(format!("note {}", ids::encode(*id))))?;
                Ok(match &n.anchor {
                    Anchor::Sheet(_) | Anchor::View(_) => Resolved { status: Status::Ok, point: None, exact: true },
                    Anchor::Edge { edge, .. } => {
                        let (status, point, exact) = lookup.middle(edge);
                        Resolved { status, point, exact }
                    }
                    Anchor::Corner { point, .. } => match lookup.corner(point) {
                        Ok((at, exact)) => Resolved { status: Status::Ok, point: Some(at), exact },
                        Err(status) => Resolved { status, point: None, exact: false },
                    },
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "note_tests.rs"]
mod tests;
