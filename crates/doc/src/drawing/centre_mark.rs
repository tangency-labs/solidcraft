//! Saved intent for manual centre marks. Geometry and reference resolution are derived separately.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;
use solidcraft_geom::paper::MAX_SIDE_MM;

use super::dimension::{Lookup, ModelEdge, Status, check_edge, edge_candidates};
use super::{Drawing, add_by_id, in_id_order, require};
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

/// Current model geometry, not saved in the drawing. The centre is in the design frame and the
/// radius is in model millimetres. A missing or split edge keeps its status and no geometry.
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
                let found = lookup.edges(&mark.edge).and_then(|found| {
                    if !found.whole {
                        return Err(Status::Ambiguous(edge_candidates(&mark.edge.body, &found)));
                    }
                    let (radius, points) = lookup.arc(&mark.edge, &found, &basis)?;
                    Ok((radius, points[0], found.exact))
                });
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
        add_by_id(&mut self.centre_marks, marks, |m| m.id);
    }

    pub fn centre_mark(&self, id: u64) -> Option<&CentreMark> {
        self.centre_mark_index(id).and_then(|i| self.centre_marks.get(i)).map(|m| &**m)
    }

    pub fn centre_mark_index(&self, id: u64) -> Option<usize> {
        self.centre_marks.binary_search_by_key(&id, |m| m.id).ok()
    }

    pub(super) fn check_centre_marks(&self) -> Result<()> {
        require(self.centre_marks.len() <= MAX_CENTRE_MARKS, || format!("a drawing has at most {MAX_CENTRE_MARKS} centre marks"))?;
        require(in_id_order(&self.centre_marks, |m| m.id), || "a drawing lists its centre marks by id".into())?;
        for mark in &self.centre_marks {
            self.check_centre_mark(mark).map_err(|e| DocError::Invalid(format!("centre mark {}: {e}", ids::encode(mark.id))))?;
        }
        Ok(())
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
