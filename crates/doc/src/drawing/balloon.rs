//! A drawing's balloons (W26, #3874): each points at a part placed in its view's model, by an edge
//! of it, to number that part on the drawing. A balloon saves what it points at by name, never a
//! picked point, as a dimension does ([`super::dimension`]): when the model changes it follows the
//! part, or says what it lost, and it never moves to another part.
//!
//! - **The part** it numbers is the occurrence its edge's body is placed through: the full path,
//!   from the top. A body of the design itself is not a placed part, so no balloon points at one.
//! - **The arrow** ends on the middle of its edge, whichever way the kernel runs the edge.
//! - **The balloon** sits at a place on paper, in millimetres from its view's centre, so it moves
//!   with the view and not with the model.
//!
//! The number a balloon shows comes from a parts list, worked out and never saved: the one it names,
//! else its sheet's only one, else the drawing's only one ([`Drawing::parts_list_for`]). It keeps the
//! balloon whatever becomes of the row.
//!
//! The rules, beside the record's: a balloon is on one of the drawing's views; its edge reference
//! keeps a dimension's rules, with an occurrence path of 1 to [`crate::MAX_PLACEMENT_DEPTH`] ids; its place
//! is on paper ([`is_paper_coordinate`]); the list it names is one of the drawing's parts lists; at
//! most [`MAX_BALLOONS`] balloons, listed by id.
//!
//! [`Document::resolve_balloons`] finds each balloon's edge again as a dimension's is found
//! ([`super::dimension::Status`]): by name, strictly, and what it can't decide it says.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::drawing_view::is_paper_coordinate;
use solidcraft_geom::paper::MAX_SIDE_MM;

use super::by_id::Record;
use super::dimension::{Lookup, ModelEdge, Status, check_edge, edge_candidates};
use super::table::{Kind, PartsList, SheetTable};
use super::{Drawing, require};
use crate::{DocError, Document, Model, Result, ids};

/// The most balloons a drawing may have.
pub const MAX_BALLOONS: usize = 10_000;

/// One balloon of a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Balloon {
    #[serde(with = "crate::ids")]
    pub id: u64,
    /// The view it points into and moves with.
    #[serde(with = "crate::ids")]
    pub view: u64,
    /// The edge its arrow points at. Its occurrence path is the part the balloon numbers.
    pub edge: ModelEdge,
    /// The balloon's centre, in millimetres of paper from its view's centre.
    pub at: [f64; 2],
    /// The parts list it names, when it names one.
    #[serde(default, with = "crate::ids::option", skip_serializing_if = "Option::is_none")]
    pub parts_list: Option<u64>,
}

impl Balloon {
    /// The part it numbers: the occurrences its edge's body is placed through, from the top.
    pub fn occurrence(&self) -> &[u64] {
        &self.edge.path
    }
}

impl Record for Balloon {
    fn id(&self) -> u64 {
        self.id
    }
}

impl Drawing {
    /// The parts list balloon `b` reads, by its id: the one it names, else its sheet's only one, else
    /// the drawing's only one. With none to go by, the lists it could name, by id.
    pub fn parts_list_for(&self, b: &Balloon) -> std::result::Result<(u64, PartsList), Vec<u64>> {
        let list = |t: &SheetTable| match t.kind {
            Kind::PartsList(list) => Some((t.id, list)),
            Kind::Revision => None,
        };
        let lists: Vec<(u64, u64, PartsList)> = self.tables.iter().filter_map(|t| list(t).map(|(id, l)| (id, t.sheet, l))).collect();
        if let Some(named) = lists.iter().find(|(id, ..)| Some(*id) == b.parts_list) {
            return Ok((named.0, named.2));
        }
        let sheet = self.view(b.view).map(|v| v.sheet);
        let mut on_sheet = lists.iter().filter(|(_, on, _)| Some(*on) == sheet);
        if let (Some(only), None) = (on_sheet.next(), on_sheet.next()) {
            return Ok((only.0, only.2));
        }
        match lists.as_slice() {
            [only] => Ok((only.0, only.2)),
            _ => Err(lists.iter().map(|(id, ..)| *id).collect()),
        }
    }

    /// The balloons' rules (the module's notes). The views and the tables are checked first, so they
    /// are in id order here.
    pub(super) fn check_balloons(&self) -> Result<()> {
        self.balloons.check(MAX_BALLOONS, "balloon", |b| self.check_balloon(b))
    }

    fn check_balloon(&self, b: &Balloon) -> Result<()> {
        require(self.view(b.view).is_some(), || format!("it is on view {}, which the drawing doesn't have", ids::encode(b.view)))?;
        if let Some(list) = b.parts_list {
            let named = || format!("it names table {}, which isn't one of the drawing's parts lists", ids::encode(list));
            require(self.tables.by_id(list).is_some_and(|t| matches!(t.kind, Kind::PartsList(_))), named)?;
        }
        check_edge(&b.edge)?;
        require(!b.edge.path.is_empty(), || "it points at a placed part: its edge's body is placed through an occurrence".into())?;
        let [x, y] = b.at;
        require(is_paper_coordinate(x) && is_paper_coordinate(y), || format!("its place is at most {MAX_SIDE_MM} mm on paper"))
    }
}

/// A balloon as the model has it now ([`Document::resolve_balloons`]): never saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub status: Status,
    /// Where its arrow ends, the middle of its edge, in the design's frame: when the edge was found.
    pub point: Option<Vec3>,
    /// Its edge was found by exact history, as one edge.
    pub exact: bool,
}

impl Document {
    /// The balloons `wanted` of `drawing` (one of this design's drawings) as `model` (this
    /// design's) has them now: each one's edge found again by name, and the middle of it. A body is
    /// looked up and its edge names worked out once for the call, however many balloons name it.
    /// `Err` only for an id the drawing doesn't have.
    pub fn resolve_balloons(&self, model: &Model, drawing: &Drawing, wanted: &[u64]) -> Result<Vec<Resolved>> {
        let mut lookup = Lookup::new(self, model);
        wanted
            .iter()
            .map(|id| {
                let b = drawing.balloons.by_id(*id).ok_or_else(|| DocError::Unknown(format!("balloon {}", ids::encode(*id))))?;
                Ok(match lookup.edges(&b.edge) {
                    // A split edge has no one middle: the pieces are listed, and none is chosen.
                    Ok(found) if !found.whole => {
                        Resolved { status: Status::Ambiguous(edge_candidates(&b.edge.body, &found)), point: None, exact: false }
                    }
                    Ok(found) => {
                        let point = found.pieces.first().and_then(|(_, line)| crate::joints::edge::middle(line)).map(|(p, _)| p);
                        let status = if point.is_some() { Status::Ok } else { Status::Lost(format!("edge `{}` has no length", b.edge.name)) };
                        Resolved { status, point, exact: found.exact }
                    }
                    Err(status) => Resolved { status, point: None, exact: false },
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "balloon_tests.rs"]
mod tests;
