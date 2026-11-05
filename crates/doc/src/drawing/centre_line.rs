//! Associative manual centre lines between two named model points.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::paper::MAX_SIDE_MM;
use solidcraft_geom::prelude::*;

use super::dimension::{Lookup, ModelPoint, Status, check_point};
use super::{Drawing, add_by_id, in_id_order, require};
use crate::{DocError, Document, Model, Result, ids};

/// Maximum number of manual centre lines in one drawing.
pub const MAX_CENTRE_LINES: usize = 10_000;

/// A line connecting projected named points and extending beyond each on paper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CentreLine {
    #[serde(with = "crate::ids")]
    pub id: u64,
    #[serde(with = "crate::ids")]
    pub view: u64,
    pub a: ModelPoint,
    pub b: ModelPoint,
    pub overshoot_mm: f64,
}

/// Current points and status, derived from the model rather than saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub status: Status,
    pub points: Option<[Vec3; 2]>,
    pub exact: bool,
}

impl Document {
    /// Resolve both points and reject a line whose ends coincide in its view.
    pub fn resolve_centre_lines(&self, model: &Model, drawing: &Drawing, wanted: &[u64]) -> Result<Vec<Resolved>> {
        let mut lookup = Lookup::new(self, model);
        wanted
            .iter()
            .map(|id| {
                let line = drawing.centre_line(*id).ok_or_else(|| DocError::Unknown(format!("centre line {}", ids::encode(*id))))?;
                let basis = drawing.resolve_view(line.view)?.basis;
                let found = lookup.corner(&line.a).and_then(|(a, exact_a)| {
                    lookup.corner(&line.b).and_then(|(b, exact_b)| {
                        if (basis.to_local(a) - basis.to_local(b)).normalized().is_none() {
                            Err(Status::Unmeasurable("the centre-line endpoints coincide in the view".into()))
                        } else {
                            Ok(([a, b], exact_a && exact_b))
                        }
                    })
                });
                Ok(match found {
                    Ok((points, exact)) => Resolved { status: Status::Ok, points: Some(points), exact },
                    Err(status) => Resolved { status, points: None, exact: false },
                })
            })
            .collect()
    }
}

impl Drawing {
    pub fn add_centre_lines(&mut self, lines: impl IntoIterator<Item = Arc<CentreLine>>) {
        add_by_id(&mut self.centre_lines, lines, |line| line.id);
    }

    pub fn centre_line(&self, id: u64) -> Option<&CentreLine> {
        self.centre_line_index(id).and_then(|i| self.centre_lines.get(i)).map(|line| &**line)
    }

    pub fn centre_line_index(&self, id: u64) -> Option<usize> {
        self.centre_lines.binary_search_by_key(&id, |line| line.id).ok()
    }

    pub(super) fn check_centre_lines(&self) -> Result<()> {
        require(self.centre_lines.len() <= MAX_CENTRE_LINES, || format!("a drawing has at most {MAX_CENTRE_LINES} centre lines"))?;
        require(in_id_order(&self.centre_lines, |line| line.id), || "a drawing lists its centre lines by id".into())?;
        for line in &self.centre_lines {
            require(self.view(line.view).is_some(), || format!("centre line {} names a missing view", ids::encode(line.id)))?;
            check_point(&line.a)?;
            check_point(&line.b)?;
            require(line.overshoot_mm.is_finite() && (0.0..=MAX_SIDE_MM).contains(&line.overshoot_mm), || {
                format!("centre-line paper overshoot is 0 to {MAX_SIDE_MM} mm")
            })?;
        }
        Ok(())
    }
}
