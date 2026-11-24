//! A solver for a sketch drawn on a curved face, and the surface geometry it reads.
//!
//! The planar solver (`crate::solver`) solves a sketch in its own plane; this module solves a
//! sketch in the chart of a curved face, where every length and angle is measured with the
//! surface's own metric and the distance between two points is a geodesic, not a straight chart
//! step. The residuals, the finite-difference Jacobian and the Levenberg–Marquardt loop are
//! transliterated here rather than shared, so the planar solver's behaviour is untouched
//! (`crates/sketch/src/solver.rs` receives no edits).

use solidcore_ops::surface_chart::SurfaceChart as OpsChart;

/// Why a geodesic or an intrinsic length cannot be had: the seam crate's own type, so the sketch
/// never re-declares the kernel's reasons and the words stay in one place.
pub use solidcore_ops::GeodesicError as SurfaceGeodesicError;

/// Where a chart point lies: the seam crate's own type, for the same reason.
pub use solidcore_ops::surface_chart::Region as SurfaceRegion;

/// A geodesic between two chart points.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceGeodesic {
    /// Samples in chart coordinates, from the start to the end.
    pub samples: Vec<[f64; 2]>,
    /// The intrinsic length, in millimetres.
    pub length: f64,
    /// The tangent direction in chart coordinates at the start.
    pub tan0: [f64; 2],
    /// The tangent direction in chart coordinates at the end.
    pub tan1: [f64; 2],
    /// Whole turns taken along u and v (0 for the first slice).
    pub windings: [i32; 2],
}

/// The surface geometry a surface solve reads: chart coordinates in, the metric, the geodesics
/// and the domain out. Implementations measure with the surface's own metric, never the chart's.
pub trait SurfaceChart {
    /// The first fundamental form at a chart point, in chart coordinates:
    /// `(E, F, G) = (S_x·S_x, S_x·S_y, S_y·S_y)`.
    fn metric(&self, x: f64, y: f64) -> Option<[f64; 3]>;

    /// The shortest geodesic between two chart points.
    fn geodesic(&self, a: [f64; 2], b: [f64; 2]) -> Result<SurfaceGeodesic, SurfaceGeodesicError>;

    /// The shortest geodesic's length between two chart points, in millimetres.
    fn distance(&self, a: [f64; 2], b: [f64; 2]) -> Result<f64, SurfaceGeodesicError> {
        self.geodesic(a, b).map(|g| g.length)
    }

    /// The surface point at a chart point, in millimetres.
    fn point(&self, x: f64, y: f64) -> Option<[f64; 3]>;

    /// Whether a chart point is inside the surface's own domain.
    fn in_domain(&self, x: f64, y: f64) -> bool;

    /// Where a chart point lies relative to the face and its surface.
    fn region(&self, x: f64, y: f64) -> SurfaceRegion;
}

/// Why a sketch cannot be solved on a curved face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceRefusal {
    /// A constraint kind (or a curve a supported constraint is drawn on) this solver does not
    /// support.
    NotSupported { kind: String, id: String },
    /// A point leaves the surface's own domain.
    OutsideSurface { id: String },
}

impl SurfaceRefusal {
    /// The user-facing words for this refusal.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            SurfaceRefusal::NotSupported { kind, .. } => {
                format!("{kind} is not supported on a curved face")
            }
            SurfaceRefusal::OutsideSurface { .. } => "the sketch leaves the surface".to_string(),
        }
    }
}

/// The ops chart as a [`SurfaceChart`]: its metric is pulled back from the surface's (u, v) to
/// chart (x, y) coordinates, and its geodesics' samples and tangents are lowered back to chart
/// coordinates, so the solver reads one consistent coordinate system.
impl<'a> SurfaceChart for OpsChart<'a> {
    fn metric(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        let m = OpsChart::metric(self, x, y)?;
        let c = self.chart();
        let (a, b) = (c.u_axis[0], c.u_axis[1]);
        let (p, q) = (c.v_axis[0], c.v_axis[1]);
        let e = m.norm(a, b) * m.norm(a, b);
        let f = m.dot((a, b), (p, q));
        let g = m.norm(p, q) * m.norm(p, q);
        Some([e, f, g])
    }

    fn geodesic(&self, a: [f64; 2], b: [f64; 2]) -> Result<SurfaceGeodesic, SurfaceGeodesicError> {
        let g = OpsChart::geodesic(self).geodesic(self.step_uv(a[0], a[1]), self.step_uv(b[0], b[1]))?;
        let c = self.chart();
        let (ua, ub) = (c.u_axis, c.v_axis);
        let det = ua[0] * ub[1] - ua[1] * ub[0];
        let direction = |d: [f64; 2]| {
            if det.is_finite() && det.abs() > 0.0 { [(ub[1] * d[0] - ub[0] * d[1]) / det, (ua[0] * d[1] - ua[1] * d[0]) / det] } else { [0.0, 0.0] }
        };
        let mut samples = Vec::with_capacity(g.samples.len());
        for &uv in &g.samples {
            match c.lower(uv) {
                Some(xy) => samples.push(xy),
                None => return Err(SurfaceGeodesicError::OffDomain),
            }
        }
        Ok(SurfaceGeodesic { samples, length: g.length, tan0: direction(g.tan0), tan1: direction(g.tan1), windings: g.windings })
    }

    fn point(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        OpsChart::point(self, x, y)
    }

    fn in_domain(&self, x: f64, y: f64) -> bool {
        OpsChart::in_domain(self, x, y)
    }

    fn region(&self, x: f64, y: f64) -> SurfaceRegion {
        OpsChart::region(self, x, y)
    }
}

pub mod solver;

pub use solver::{SurfaceSolveReport, SurfaceSolveStatus, first_refusal, surface_solve};

#[cfg(test)]
mod tests;
