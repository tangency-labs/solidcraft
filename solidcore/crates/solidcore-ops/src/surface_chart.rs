//! A chart on a body face, for a sketch: its origin and metric-orthonormal axes in the surface's
//! own (u, v), its point, metric and trim region, and the face's (u, v) of a chart point.
//!
//! The chart itself is the kernel Chart; this module sequences the record, the face and the
//! query and converts their answers. No arithmetic of its own.
//!
//! The chart lives in the surface's STEP (u, v), the canonical metric space; the face's trim
//! stays in the face's own (u, v), reached through the face's surface point. The guard runs in
//! the surface's (u, v) first: a point outside the surface domain is off the surface; a point
//! inside it but outside the face's trim is off the face.

use solidcore::eval::{Chart as KernelChart, GeodesicError, GeodesicSolver, Metric, SurfaceEval};
use solidcore::Vec3 as EvalVec3;
use solidcore_record::brep::{Brep, Surface as RecordSurface};
use solidcore_record::Vec3 as RecordVec3;

use crate::error::{Error, Result};
use crate::face_surface::{Exact, FaceSurface};
use crate::query::Query;

/// Where a chart point lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// On the face, inside its trim.
    OnFace,
    /// On the surface, outside the face's trim.
    OffFace,
    /// Outside the surface's parameter domain.
    OffSurface,
}

/// The kernel's refusal of a geodesic, as an ops error: the operation and the kernel's reason.
fn refusal(error: GeodesicError) -> Error {
    Error::Refused {
        operation: "geodesic",
        reason: match error {
            GeodesicError::Ambiguous => "the shortest geodesic is ambiguous",
            GeodesicError::Singular => "the surface is singular on the geodesic",
            GeodesicError::OffDomain => "the geodesic leaves the surface",
            GeodesicError::NotConverged => "the geodesic did not converge",
            GeodesicError::Unsupported => "the geodesic is not supported on this surface",
        },
    }
}

/// A chart on a body face.
pub struct SurfaceChart<'a> {
    record: &'a Brep,
    face: usize,
    surface: SurfaceEval,
    chart: KernelChart,
    on: FaceSurface,
    tolerance: f64,
}

impl<'a> SurfaceChart<'a> {
    /// A chart on face of record, at the point pick on it, with tolerance in millimetres.
    ///
    /// # Errors
    ///
    /// `Error::Argument` for a face past the record's, a pick not on the face, or a tolerance
    /// that is not positive; `Error::Unsupported` for a face whose surface the evaluator does
    /// not take, or a singular point.
    pub fn build(
        record: &'a Brep,
        face: usize,
        pick: [f64; 3],
        tolerance: f64,
    ) -> Result<SurfaceChart<'a>> {
        if !(tolerance.is_finite() && tolerance > 0.0) {
            return Err(Error::Argument);
        }
        let (surface, on) = Self::surface_of(record, face)?;
        let p = RecordVec3::new(pick[0], pick[1], pick[2]);
        let query = Query::new(record);
        let face_uv = query.face_uv(face, p, tolerance).ok_or(Error::Argument)?;
        let normal = on.normal(face_uv)?;
        let reference = EvalVec3::new(normal[0], normal[1], normal[2]);
        let chart = KernelChart::build(&surface, face_uv, reference)
            .ok_or(Error::Unsupported("a singular face at the pick"))?;
        Ok(SurfaceChart {
            record,
            face,
            surface,
            chart,
            on,
            tolerance,
        })
    }

    /// A chart on a face from its saved origin and axes in the surface's (u, v), the one a design
    /// stored: the same chart its sketch was drawn in, so chart coordinates keep their meaning
    /// across a reload.
    ///
    /// # Errors
    ///
    /// `Error::Argument` for a face past the record's, a number that is not finite, a degenerate
    /// axis pair or a tolerance that is not positive; `Error::Unsupported` for a face whose
    /// surface the evaluator does not take, or a singular origin.
    pub fn from_axes(
        record: &'a Brep,
        face: usize,
        origin: [f64; 2],
        u_axis: [f64; 2],
        v_axis: [f64; 2],
        tolerance: f64,
    ) -> Result<SurfaceChart<'a>> {
        if !(tolerance.is_finite() && tolerance > 0.0) {
            return Err(Error::Argument);
        }
        let numbers = [
            origin[0], origin[1], u_axis[0], u_axis[1], v_axis[0], v_axis[1],
        ];
        if !numbers.iter().all(|v| v.is_finite()) {
            return Err(Error::Argument);
        }
        let det = u_axis[0] * v_axis[1] - u_axis[1] * v_axis[0];
        if !(det.is_finite() && det.abs() > 1e-15) {
            return Err(Error::Argument);
        }
        let (surface, on) = Self::surface_of(record, face)?;
        if surface.metric(origin[0], origin[1]).is_none() {
            return Err(Error::Unsupported("a singular chart origin"));
        }
        Ok(SurfaceChart {
            record,
            face,
            surface,
            chart: KernelChart {
                origin,
                u_axis,
                v_axis,
            },
            on,
            tolerance,
        })
    }

    /// The evaluator and the face wrapper of one face of the record.
    fn surface_of(record: &Brep, face: usize) -> Result<(SurfaceEval, FaceSurface)> {
        let on = FaceSurface::of(record, face)?;
        let record_face = record.faces.get(face).ok_or(Error::Argument)?;
        let record_surface: &RecordSurface = if on.trims() {
            record_face
                .nurbs
                .as_ref()
                .ok_or(Error::Unsupported("a face without its NURBS form"))?
        } else {
            match on.exact() {
                Exact::Label(label) => label,
                _ => record_face
                    .nurbs
                    .as_ref()
                    .ok_or(Error::Unsupported("a face without its NURBS form"))?,
            }
        };
        let converted = solidcore_convert::surface(record_surface)
            .map_err(|_| Error::Unsupported("a surface the evaluator does not take"))?;
        let surface = SurfaceEval::new(&converted)
            .map_err(|_| Error::Unsupported("a surface the evaluator does not take"))?;
        Ok((surface, on))
    }

    /// The kernel chart: origin and axes in the surface's (u, v).
    #[must_use]
    pub fn chart(&self) -> &KernelChart {
        &self.chart
    }

    /// The chart point's (u, v) on the face's surface.
    #[must_use]
    pub fn step_uv(&self, x: f64, y: f64) -> [f64; 2] {
        self.chart.lift(x, y)
    }

    /// The surface point at a chart point, in millimetres.
    #[must_use]
    pub fn point(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        let p = self.surface.point(self.step_uv(x, y))?;
        Some([p.x, p.y, p.z])
    }

    /// The surface metric at a chart point.
    #[must_use]
    pub fn metric(&self, x: f64, y: f64) -> Option<Metric> {
        let [u, v] = self.step_uv(x, y);
        self.surface.metric(u, v)
    }

    /// The face's (u, v) of a chart point, where the point is on the face's surface.
    #[must_use]
    pub fn face_uv(&self, x: f64, y: f64) -> Option<[f64; 2]> {
        let p = self.point(x, y)?;
        Query::new(self.record).face_uv(
            self.face,
            RecordVec3::new(p[0], p[1], p[2]),
            self.tolerance,
        )
    }

    /// Whether a chart point's surface (u, v) is in the surface's own domain, within the
    /// chart tolerance: a periodic direction never leaves it (the seam wraps), a bounded one
    /// may be reached from either side up to the tolerance.
    #[must_use]
    pub fn in_domain(&self, x: f64, y: f64) -> bool {
        let uv = self.step_uv(x, y);
        let domain = self.surface.domain();
        [0, 1].into_iter().all(|k| {
            if domain.period[k].is_some() {
                return true;
            }
            domain.range[k]
                .is_none_or(|(a, b)| uv[k] >= a - self.tolerance && uv[k] <= b + self.tolerance)
        })
    }

    /// Where a chart point lies relative to the face and its surface: off-surface first (the
    /// surface's own domain), then off-face (the face's trim).
    #[must_use]
    pub fn region(&self, x: f64, y: f64) -> Region {
        if !self.in_domain(x, y) {
            return Region::OffSurface;
        }
        let Some(p) = self.point(x, y) else {
            return Region::OffSurface;
        };
        let on = Query::new(self.record).face_contains(
            self.face,
            RecordVec3::new(p[0], p[1], p[2]),
            self.tolerance,
        );
        if on {
            Region::OnFace
        } else {
            Region::OffFace
        }
    }

    /// A geodesic solver over this chart's surface, at the chart's tolerance: the shortest path
    /// between two chart points, its length, and the intrinsic quantities a sketch needs.
    #[must_use]
    pub fn geodesic(&self) -> GeodesicSolver<'_> {
        GeodesicSolver::new(&self.surface, self.tolerance)
    }

    /// The shortest path's length between two chart points, in millimetres: the surface's own
    /// distance, measured by the kernel's geodesic solver.
    ///
    /// # Errors
    /// [`Error::Argument`] for a point outside the surface's domain; [`Error::Refused`] for a
    /// path the kernel cannot find (singular, ambiguous, off the domain, or not converged).
    pub fn distance(&self, a: [f64; 2], b: [f64; 2]) -> Result<f64> {
        if !self.in_domain(a[0], a[1]) || !self.in_domain(b[0], b[1]) {
            return Err(Error::Argument);
        }
        GeodesicSolver::new(&self.surface, self.tolerance)
            .distance(self.step_uv(a[0], a[1]), self.step_uv(b[0], b[1]))
            .map_err(refusal)
    }

    /// The chart coordinates of a face (u, v), by its point projected back onto the surface.
    #[must_use]
    pub fn lower_face_uv(&self, face_uv: [f64; 2]) -> Option<[f64; 2]> {
        let p = self.on.point(face_uv).ok()?;
        let uv = self.surface.project(
            EvalVec3::new(p[0], p[1], p[2]),
            Some(face_uv),
            self.tolerance,
        )?;
        self.chart.lower(uv)
    }
}
