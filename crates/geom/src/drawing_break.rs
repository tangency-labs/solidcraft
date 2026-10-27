//! Parallel view breaks (W36): a piecewise translation after uniform view scaling.
//!
//! Bands are in the original view plane's model millimetres along the normal to the
//! break lines. Gaps are paper millimetres. Removed model points and empty paper gaps
//! have no image/preimage. Measurements must use the original geometry, never this map.
//! This first slice handles parallel, disjoint bands and exact straight-line clipping;
//! it neither changes a model nor creates persistent drawing objects.
//!
//! Classification uses a roundoff budget from the scaled break coordinates, accumulated
//! shortening and maximum paper extent (16 machine epsilons of their combined magnitude).
//! Bands, retained spans and paper gaps must exceed four such budgets; numerically
//! indistinguishable definitions are refused. Centre view-plane coordinates
//! near the geometry: the input caps are not a numerical-accuracy certificate.

use serde::{Deserialize, Serialize};

use crate::drawing_view::Scale;
use crate::paper::MAX_COORDINATE_MM;
use crate::{EPS, Vec2};

/// Bound work before sorting or producing pieces.
pub const MAX_BREAKS: usize = 64;
const MAX_MODEL_MM: f64 = 1e9;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("view break: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

/// Two parallel lines in the unbroken view, and the gap to leave on paper.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    pub start: f64,
    pub end: f64,
    pub gap_mm: f64,
}

/// A checked map. Construction normalises line order and sorts the bands.
#[derive(Clone, Debug)]
pub struct BreakMap {
    normal: Vec2,
    factor: f64,
    bands: Vec<Band>,
    paper_tolerance: f64,
}

fn point_ok(p: Vec2, limit: f64) -> bool {
    p.is_finite() && p.x.abs() <= limit && p.y.abs() <= limit
}

fn boundary_tolerance(a: f64, b: f64) -> f64 {
    16.0 * f64::EPSILON * a.abs().max(b.abs()).max(1.0)
}

fn at_boundary(value: f64, boundary: f64, tolerance: f64) -> bool {
    (value - boundary).abs() <= tolerance.max(boundary_tolerance(value, boundary))
}

fn strictly_inside(value: f64, low: f64, high: f64, tolerance: f64) -> bool {
    value > low && value < high && !at_boundary(value, low, tolerance) && !at_boundary(value, high, tolerance)
}

impl BreakMap {
    /// `line_angle` is the break line's angle, anticlockwise in radians in the view plane.
    pub fn new(line_angle: f64, scale: Scale, bands: &[Band]) -> Result<Self> {
        if !line_angle.is_finite() || line_angle.abs() > 1e6 {
            return Err(Invalid("line angle must be finite and within 1e6 radians"));
        }
        if bands.len() > MAX_BREAKS {
            return Err(Invalid("more than 64 breaks"));
        }
        let factor = scale.factor();
        let mut checked = Vec::with_capacity(bands.len());
        for band in bands {
            let start = band.start.min(band.end);
            let end = band.start.max(band.end);
            if ![band.start, band.end].into_iter().all(|v| v.is_finite() && v.abs() <= MAX_MODEL_MM)
                || end - start <= EPS
                || !band.gap_mm.is_finite()
                || band.gap_mm <= EPS
                || band.gap_mm > MAX_COORDINATE_MM
                || band.gap_mm >= (end - start) * factor
            {
                return Err(Invalid("finite, distinct break lines must remove more paper than the positive gap"));
            }
            checked.push(Band { start, end, gap_mm: band.gap_mm });
        }
        checked.sort_by(|a, b| a.start.total_cmp(&b.start));
        let shortening = checked.iter().fold(0.0, |sum, b| sum + ((b.end - b.start) * factor - b.gap_mm));
        let extent = checked.iter().fold(MAX_COORDINATE_MM, |extent, b| extent.max(b.start.abs() * factor).max(b.end.abs() * factor));
        // A small displayed gap can follow a large subtraction. Its classification must
        // account for the original coordinates and translation, not just its paper position.
        let paper_tolerance = 16.0 * f64::EPSILON * (extent + shortening);
        let model_tolerance = paper_tolerance / factor;
        if checked.iter().any(|b| b.end - b.start <= 4.0 * model_tolerance) {
            return Err(Invalid("break span is numerically indistinguishable"));
        }
        if checked.windows(2).any(|pair| pair.first().zip(pair.get(1)).is_some_and(|(a, b)| b.start - a.end <= 4.0 * model_tolerance)) {
            return Err(Invalid("breaks overlap, touch or have an indistinguishable retained span"));
        }
        if checked.iter().any(|band| band.gap_mm <= 4.0 * paper_tolerance) {
            return Err(Invalid("paper gap is indistinguishable at these coordinates"));
        }
        let (sin, cos) = line_angle.sin_cos();
        Ok(Self { normal: Vec2::new(-sin, cos), factor, bands: checked, paper_tolerance })
    }

    pub fn normal(&self) -> Vec2 {
        self.normal
    }

    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    /// Total shortening, in paper millimetres.
    pub fn shortening_mm(&self) -> f64 {
        self.bands.iter().fold(0.0, |sum, b| sum + ((b.end - b.start) * self.factor - b.gap_mm))
    }

    /// The two displayed break-line coordinates along the normal, in paper mm.
    pub fn paper_gaps(&self) -> Vec<[f64; 2]> {
        let mut shift = 0.0;
        self.bands
            .iter()
            .map(|b| {
                let low = b.start * self.factor - shift;
                shift += (b.end - b.start) * self.factor - b.gap_mm;
                [low, low + b.gap_mm]
            })
            .collect()
    }

    fn shift_at(&self, coordinate: f64) -> Option<f64> {
        let mut shift = 0.0;
        let tolerance = self.paper_tolerance / self.factor;
        for band in &self.bands {
            if strictly_inside(coordinate, band.start, band.end, tolerance) {
                return None;
            }
            if coordinate >= band.end || at_boundary(coordinate, band.end, tolerance) {
                shift += (band.end - band.start) * self.factor - band.gap_mm;
            } else {
                break;
            }
        }
        Some(shift)
    }

    fn translated(&self, point: Vec2, shift: f64) -> Result<Vec2> {
        let result = point * self.factor - self.normal * shift;
        if !point_ok(result, MAX_COORDINATE_MM) {
            return Err(Invalid("mapped point exceeds paper coordinate limits"));
        }
        Ok(result)
    }

    /// `None` inside a removed span; both boundary lines remain drawable.
    pub fn to_paper(&self, point: Vec2) -> Result<Option<Vec2>> {
        if !point_ok(point, MAX_MODEL_MM) {
            return Err(Invalid("model point is non-finite or exceeds 1e9 mm"));
        }
        self.shift_at(point.dot(self.normal)).map(|shift| self.translated(point, shift)).transpose()
    }

    /// Inverse for picking: a paper gap is empty, not a guessed model coordinate.
    pub fn to_model(&self, paper: Vec2) -> Result<Option<Vec2>> {
        if !point_ok(paper, MAX_COORDINATE_MM) {
            return Err(Invalid("paper point is non-finite or excessive"));
        }
        let coordinate = paper.dot(self.normal);
        let mut shift = 0.0;
        for band in &self.bands {
            let low = band.start * self.factor - shift;
            let high = low + band.gap_mm;
            if strictly_inside(coordinate, low, high, self.paper_tolerance) {
                return Ok(None);
            }
            if coordinate >= high || at_boundary(coordinate, high, self.paper_tolerance) {
                shift += (band.end - band.start) * self.factor - band.gap_mm;
            } else {
                break;
            }
        }
        let model = (paper + self.normal * shift) * (1.0 / self.factor);
        if !point_ok(model, MAX_MODEL_MM) {
            return Err(Invalid("inverse point exceeds model coordinate limits"));
        }
        Ok(Some(model))
    }

    /// Clip a straight model segment against every removed band, then translate retained
    /// pieces exactly. Both ends of each piece use the same translation so roundoff at
    /// a clipping boundary cannot choose the other side of a break.
    pub fn line(&self, a: Vec2, b: Vec2) -> Result<Vec<[Vec2; 2]>> {
        if !point_ok(a, MAX_MODEL_MM) || !point_ok(b, MAX_MODEL_MM) {
            return Err(Invalid("line endpoint is non-finite or exceeds 1e9 mm"));
        }
        let delta = b - a;
        if delta.len() <= EPS {
            return Err(Invalid("line endpoints coincide"));
        }
        let origin = a.dot(self.normal);
        let step = delta.dot(self.normal);
        let mut cuts = vec![0.0, 1.0];
        if step != 0.0 {
            for band in &self.bands {
                for boundary in [band.start, band.end] {
                    let t = (boundary - origin) / step;
                    if t > 0.0 && t < 1.0 {
                        cuts.push(t);
                    }
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let mut pieces = Vec::new();
        for pair in cuts.windows(2) {
            if let Some((&start, &end)) = pair.first().zip(pair.get(1))
                && let Some(shift) = self.shift_at(origin + step * (start + (end - start) * 0.5))
            {
                pieces.push([self.translated(a + delta * start, shift)?, self.translated(a + delta * end, shift)?]);
            }
        }
        Ok(pieces)
    }
}

#[cfg(test)]
#[path = "drawing_break_tests.rs"]
mod tests;
