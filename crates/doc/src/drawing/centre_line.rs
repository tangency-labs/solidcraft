//! Associative manual centre lines between two named model points.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::paper::MAX_SIDE_MM;
use solidcraft_geom::prelude::*;

use super::by_id::Record;
use super::dimension::{Lookup, ModelPoint, Status, check_point};
use super::{Drawing, require};
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

impl Record for CentreLine {
    fn id(&self) -> u64 {
        self.id
    }
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
        self.centre_lines.add(lines);
    }

    pub fn centre_line(&self, id: u64) -> Option<&CentreLine> {
        self.centre_lines.by_id(id)
    }

    pub fn centre_line_index(&self, id: u64) -> Option<usize> {
        self.centre_lines.index_of(id)
    }

    pub(super) fn check_centre_lines(&self) -> Result<()> {
        self.centre_lines.check(MAX_CENTRE_LINES, "centre line", |line| {
            require(self.view(line.view).is_some(), || "names a missing view".into())?;
            check_point(&line.a)?;
            check_point(&line.b)?;
            require(line.overshoot_mm.is_finite() && (0.0..=MAX_SIDE_MM).contains(&line.overshoot_mm), || {
                format!("centre-line paper overshoot is 0 to {MAX_SIDE_MM} mm")
            })
        })
    }
}
