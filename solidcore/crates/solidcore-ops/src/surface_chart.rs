//! A chart on a body face, for a sketch: its origin and metric-orthonormal axes in the face's
//! own (u, v), its point, metric and trim region, and the face's (u, v) of a chart point.
//!
//! The chart itself is the kernel Chart; this module sequences the record, the face and the
//! query and converts their answers. No arithmetic of its own.
//!
//! The chart lives in the face's (u, v), the one parameterisation in which the face's trim and
//! the surface metric are both known. A point outside the face's trim but inside the surface
//! domain is off the face; a point outside the surface domain is off the surface.

use solidcore::eval::{Chart as KernelChart, Metric, SurfaceEval};
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

    /// The kernel chart: origin and axes in the face's (u, v).
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

    /// Where a chart point lies relative to the face and its surface.
    #[must_use]
    pub fn region(&self, x: f64, y: f64) -> Region {
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
