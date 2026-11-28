//! Saved intent for manual centre marks. Geometry and reference resolution are derived separately.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::paper::MAX_SIDE_MM;

use super::by_id::Record;
use super::dimension::{Lookup, ModelEdge, Status, check_edge};
use super::{Drawing, require};
use crate::{DocError, Document, Model, Result, ids};

/// Maximum number of manual centre marks in a drawing.
pub const MAX_CENTRE_MARKS: usize = 10_000;

/// A manual mark attached to an edge of the model shown in a view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CentreMark {
    #[serde(with = "crate::ids")]
    pub id: u64,
    #[serde(with = "crate::ids")]
    pub view: u64,
    pub edge: ModelEdge,
    /// Extension beyond the circle at each end, in millimetres on paper.
    pub overshoot_mm: f64,
}

impl Record for CentreMark {
    fn id(&self) -> u64 {
        self.id
    }
}

/// Current model geometry, not saved in the drawing. The centre is in the design frame and the
/// radius is in model millimetres. A missing or ambiguous edge keeps its status and no geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub status: Status,
    pub centre: Option<Vec3>,
    pub radius_mm: Option<f64>,
    pub exact: bool,
}

impl Document {
    /// Resolve marks through the same named-edge and face-on arc rules as radial dimensions.
    pub fn resolve_centre_marks(&self, model: &Model, drawing: &Drawing, wanted: &[u64]) -> Result<Vec<Resolved>> {
        let mut lookup = Lookup::new(self, model);
        wanted
            .iter()
            .map(|id| {
                let mark = drawing.centre_mark(*id).ok_or_else(|| DocError::Unknown(format!("centre mark {}", ids::encode(*id))))?;
                let basis = drawing.resolve_view(mark.view)?.basis;
                let found = lookup.whole_arc(&mark.edge, &basis).map(|(radius, points, exact)| (radius, points[0], exact));
                Ok(match found {
                    Ok((radius, centre, exact)) => Resolved { status: Status::Ok, centre: Some(centre), radius_mm: Some(radius), exact },
                    Err(status) => Resolved { status, centre: None, radius_mm: None, exact: false },
                })
            })
            .collect()
    }
}

impl Drawing {
    /// Add marks in id order, like drawing dimensions.
    pub fn add_centre_marks(&mut self, marks: impl IntoIterator<Item = Arc<CentreMark>>) {
        self.centre_marks.add(marks);
    }

    pub fn centre_mark(&self, id: u64) -> Option<&CentreMark> {
        self.centre_marks.by_id(id)
    }

    pub fn centre_mark_index(&self, id: u64) -> Option<usize> {
        self.centre_marks.index_of(id)
    }

    pub(super) fn check_centre_marks(&self) -> Result<()> {
        self.centre_marks.check(MAX_CENTRE_MARKS, "centre mark", |mark| self.check_centre_mark(mark))
    }

    fn check_centre_mark(&self, mark: &CentreMark) -> Result<()> {
        require(self.view(mark.view).is_some(), || format!("it is on view {}, which the drawing doesn't have", ids::encode(mark.view)))?;
        check_edge(&mark.edge)?;
        require(mark.overshoot_mm.is_finite() && (0.0..=MAX_SIDE_MM).contains(&mark.overshoot_mm), || {
            format!("its paper overshoot is 0 to {MAX_SIDE_MM} mm")
        })
    }
}

#[cfg(test)]
#[path = "centre_mark_tests.rs"]
mod tests;
