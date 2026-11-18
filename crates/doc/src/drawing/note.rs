//! A drawing's notes (W19e, #4896): text on a sheet, in its fields and its words, that may point at
//! the model with leaders.
//!
//! - **The anchor** says what a note belongs to: a sheet, a view, or an edge or a corner of a view's
//!   model, saved by name as a dimension's is ([`super::dimension`]). It is one type so that other
//!   symbols placed on a drawing can use it too.
//! - **The place** is on paper: in millimetres from the sheet's lower-left corner for a note on a
//!   sheet, and from the view's centre for any other, so that note moves with its view.
//! - **The content** is the title block's text with fields ([`Content`]), so a sheet's number works
//!   in a note. It reads what a note can name: never an answer given when a block was placed, or a
//!   table's row.
//! - **The branches** (#13059) are more leaders of a note on a view, each to an edge or a corner of
//!   the view's model, saved by name as the anchor's are. A note on a view alone has its first branch
//!   as its first leader.
//! - **The leaders** are worked out, never saved, from where the anchor and each branch stand with
//!   the model now ([`Document::resolve_notes`]). A lost reference loses only its own leader: the note
//!   and its text stay where they are.
//!
//! The rules, beside the record's: its sheet or view is one of the drawing's; an edge or a corner
//! keeps a dimension's reference rules; branches only on a note on a view, and at most
//! [`MAX_LEADERS`] leaders in all ([`leaders_refused`]); its place is on paper
//! ([`is_paper_coordinate`]); its text height is [`CAP_MM`]; at most [`MAX_NOTES`] notes, listed by
//! id.

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::cell::{Content, Part, Source};
use solidcraft_geom::drawing_view::is_paper_coordinate;
use solidcraft_geom::paper::MAX_SIDE_MM;

use super::by_id::Record;
use super::dimension::{Lookup, ModelEdge, ModelPoint, Reference, Status, check_edge, check_point};
use super::{Drawing, require};
use crate::{DocError, Document, Model, Result, ids};

/// The most notes a drawing may have.
pub const MAX_NOTES: usize = 10_000;
/// The most leaders one note may have: its anchor's, when it points at the model, and its branches.
pub const MAX_LEADERS: usize = 16;
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

    /// The model reference its leader points at, if it points at one.
    pub fn reference(&self) -> Option<Reference<'_>> {
        match self {
            Anchor::Sheet(_) | Anchor::View(_) => None,
            Anchor::Edge { edge, .. } => Some(Reference::Edge(edge)),
            Anchor::Corner { point, .. } => Some(Reference::Point(point)),
        }
    }

    /// The occurrence path of the model reference its leader points at, if it points at one.
    pub fn path(&self) -> Option<&[u64]> {
        self.reference().map(Reference::path)
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

/// One more leader of a note on a view (#13059).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Branch {
    /// It ends on the edge's middle.
    Edge(ModelEdge),
    /// It ends on the corner or the centre.
    Corner(ModelPoint),
}

impl Branch {
    /// The occurrence path of the model reference it points at.
    pub fn path(&self) -> &[u64] {
        match self {
            Branch::Edge(edge) => &edge.path,
            Branch::Corner(point) => &point.path,
        }
    }
}

/// Why a note can't have `branches` branches and `leaders` leaders in all, when it can't: branches
/// point at a view's model, so a note on a sheet has none, and a note has at most [`MAX_LEADERS`].
/// The record and the commands share it, so a command refuses before it looks anything up.
pub fn leaders_refused(on_view: bool, branches: usize, leaders: usize) -> Option<String> {
    if branches > 0 && !on_view {
        Some("a note on a sheet has no branches: it points at no model".into())
    } else if leaders > MAX_LEADERS {
        Some(format!("a note has at most {MAX_LEADERS} leaders, its anchor's included"))
    } else {
        None
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
    /// Its leaders beside its anchor's, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<Branch>,
}

impl Note {
    /// The model references its leaders point at: the anchor's, then each branch's.
    pub fn references(&self) -> impl Iterator<Item = Reference<'_>> {
        fn branch(branch: &Branch) -> Reference<'_> {
            match branch {
                Branch::Edge(edge) => Reference::Edge(edge),
                Branch::Corner(point) => Reference::Point(point),
            }
        }
        self.anchor.reference().into_iter().chain(self.branches.iter().map(branch))
    }

    /// The occurrence paths of the model references its leaders point at ([`Note::references`]).
    pub fn paths(&self) -> impl Iterator<Item = &[u64]> {
        self.references().map(Reference::path)
    }

    /// Whether one of its leaders points at the model ([`Note::references`]).
    pub fn on_model(&self) -> bool {
        self.references().next().is_some()
    }
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

    /// Where in `sheets` the sheet a note with `anchor` is on sits: its own sheet, or its view's.
    pub fn sheet_of(&self, anchor: &Anchor) -> Option<usize> {
        let sheet = match anchor {
            Anchor::Sheet(sheet) => *sheet,
            _ => self.view(anchor.view()?)?.sheet,
        };
        self.sheets.iter().position(|one| one.id == sheet)
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
        if let Some(why) = leaders_refused(n.anchor.view().is_some(), n.branches.len(), n.paths().count()) {
            return Err(DocError::Invalid(why));
        }
        for branch in &n.branches {
            match branch {
                Branch::Edge(edge) => check_edge(edge)?,
                Branch::Corner(point) => check_point(point)?,
            }
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

/// Where one leader of a note stands with the model now.
#[derive(Clone, Debug, PartialEq)]
pub struct End {
    pub status: Status,
    /// Where it ends, in the design's frame: when it is found.
    pub point: Option<Vec3>,
    /// What it points at was found by exact history, as one edge.
    pub exact: bool,
}

impl End {
    /// The leader of no model reference: a note on a sheet or a view alone.
    const NONE: End = End { status: Status::Ok, point: None, exact: true };

    fn edge(lookup: &mut Lookup<'_>, edge: &ModelEdge) -> End {
        let (status, point, exact) = lookup.middle(edge);
        End { status, point, exact }
    }

    fn corner(lookup: &mut Lookup<'_>, point: &ModelPoint) -> End {
        match lookup.corner(point) {
            Ok((at, exact)) => End { status: Status::Ok, point: Some(at), exact },
            Err(status) => End { status, point: None, exact: false },
        }
    }
}

/// A note as the model has it now ([`Document::resolve_notes`]): never saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    /// Its anchor's leader: always [`Status::Ok`], with no point, for a note on a sheet or a view,
    /// which reads no model. The note's status is this one's: a lost branch loses only itself.
    pub anchor: End,
    /// Each branch's, in order.
    pub branches: Vec<End>,
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
                let anchor = match &n.anchor {
                    Anchor::Sheet(_) | Anchor::View(_) => End::NONE,
                    Anchor::Edge { edge, .. } => End::edge(&mut lookup, edge),
                    Anchor::Corner { point, .. } => End::corner(&mut lookup, point),
                };
                let mut branch = |branch: &Branch| match branch {
                    Branch::Edge(edge) => End::edge(&mut lookup, edge),
                    Branch::Corner(point) => End::corner(&mut lookup, point),
                };
                Ok(Resolved { anchor, branches: n.branches.iter().map(&mut branch).collect() })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "note_tests.rs"]
mod tests;
