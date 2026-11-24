//! A sketch on a curved face: the face it is attached to, the chart its (u, v) live in, and
//! the sketch itself.

use serde::{Deserialize, Serialize};

use crate::config::ConfigFaceTarget;

/// The chart a surface sketch is stored in: the surface's STEP (u, v) origin and two
/// metric-orthonormal axes, with the tolerance the chart was built at. The face's trim stays in
/// the face's own (u, v); the chart is the canonical metric space the sketch solves in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceChart {
    /// The origin in the surface's (u, v).
    pub origin: [f64; 2],
    /// The chart u axis in the surface's (u, v).
    pub u_axis: [f64; 2],
    /// The chart v axis in the surface's (u, v).
    pub v_axis: [f64; 2],
    /// The tolerance the chart was built at, in millimetres.
    pub tolerance: f64,
}

impl SurfaceChart {
    /// Whether every number is finite and the tolerance is positive.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.origin.iter().chain(&self.u_axis).chain(&self.v_axis).all(|v| v.is_finite()) && self.tolerance.is_finite() && self.tolerance > 0.0
    }
}

/// A sketch attached to a curved face: the face certificate and the chart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRef {
    /// The face the sketch follows.
    pub target: ConfigFaceTarget,
    /// The chart its (u, v) live in.
    pub chart: SurfaceChart,
}

// The surface reference is not walked yet: its target and chart hold no design expressions or
// sketch references for the walks to visit. The attachment pass replaces this.
impl crate::walk::Walk for SurfaceRef {}

impl SurfaceRef {
    /// Whether the target and the chart are valid.
    ///
    /// # Errors
    ///
    /// The target's own validation, or a chart that is not finite or has no positive tolerance.
    pub fn validate(&self) -> crate::Result<()> {
        self.target.validate()?;
        if !self.chart.is_valid() {
            return Err(crate::DocError::Invalid("invalid surface chart".into()));
        }
        Ok(())
    }
}

/// Where a saved surface reference points now, against an evaluated model.
pub enum SurfaceAttachment<'a> {
    /// The face is found and its chart rebuilt from the saved axes: the chart its sketch was
    /// drawn in.
    Healthy {
        /// The face, as an index in the attached body.
        face: usize,
        /// The chart, on the attached body's own record.
        chart: Box<solidcore_ops::surface_chart::SurfaceChart<'a>>,
    },
    /// The face is gone, or no longer carries this chart: why, for the user.
    Lost {
        /// The words to show.
        message: String,
    },
}

impl crate::Document {
    /// Where `surface` points now against `state`: the chart its sketch solves in, or why the face
    /// it was drawn on is no longer there. A face split into pieces resolves to the first piece.
    #[must_use]
    pub fn surface_attachment<'a>(&self, surface: &SurfaceRef, state: &'a crate::ModelState) -> SurfaceAttachment<'a> {
        let lost = |message: String| SurfaceAttachment::Lost { message };
        let Some(body) = state.body(&surface.target.body) else {
            return lost(format!("attached body `{}` no longer exists", surface.target.body));
        };
        if !surface.chart.is_valid() {
            return lost("the sketch's chart is not a valid frame".into());
        }
        let (indices, _note) = match self.anchored_face_indices(state, body, &surface.target) {
            Ok(found) => found,
            Err(error) => return lost(error.to_string()),
        };
        let Some(&face) = indices.first() else {
            return lost("attached face no longer exists".into());
        };
        let chart = &surface.chart;
        match solidcore_ops::surface_chart::SurfaceChart::from_axes(
            body.body.record(),
            face,
            chart.origin,
            chart.u_axis,
            chart.v_axis,
            chart.tolerance,
        ) {
            Ok(chart) => SurfaceAttachment::Healthy { face, chart: Box::new(chart) },
            Err(error) => lost(format!("the attached face no longer carries this chart: {error}")),
        }
    }
}

/// A sketch on a curved face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceSketch {
    /// The face and chart.
    pub surface: SurfaceRef,
    /// The sketch's points, curves and constraints.
    pub sketch: solidcraft_sketch::Sketch,
}

impl SurfaceSketch {
    /// Whether the reference is valid.
    ///
    /// # Errors
    ///
    /// The surface reference's validation.
    pub fn validate(&self) -> crate::Result<()> {
        self.surface.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart() -> SurfaceChart {
        SurfaceChart { origin: [0.3, 1.2], u_axis: [0.6, 0.8], v_axis: [-0.8, 0.6], tolerance: 1e-6 }
    }

    #[test]
    fn a_surface_chart_round_trips_and_validates() {
        let chart = chart();
        let text = serde_json::to_string(&chart).expect("json");
        let back: SurfaceChart = serde_json::from_str(&text).expect("back");
        assert_eq!(back, chart);
        assert!(chart.is_valid());
    }

    #[test]
    fn a_non_finite_chart_is_refused() {
        let mut chart = chart();
        chart.u_axis[0] = f64::NAN;
        assert!(!chart.is_valid());
    }

    /// The saved chart is found again on the body: the same chart coordinates mean the same point
    /// and the same region, and a body that is gone is Lost rather than a stale chart.
    #[test]
    fn a_saved_chart_is_found_again_on_the_body() {
        use crate::Dir;
        use crate::document::{FeatureKind, Operation};
        use solidcore_ops::face_surface::{Exact, FaceSurface};
        use solidcore_record::Vec3;
        use solidcore_record::brep::Surface;

        let mut doc = crate::Document::new("p");
        doc.add_feature(
            FeatureKind::Cylinder {
                base: crate::point_expr(Vec3::ZERO),
                axis: Dir(Vec3::Z),
                radius: "5".into(),
                height: "10".into(),
                operation: Operation::NewBody,
                references: Vec::new(),
            },
            None,
        )
        .expect("a cylinder feature");
        let mut model = crate::Model::new();
        model.evaluate(&doc);
        let state = model.state();
        let body = state.bodies.first().expect("a body");
        let record = body.body.record();
        let face = (0..record.faces.len())
            .find(|&f| FaceSurface::of(record, f).is_ok_and(|s| matches!(s.exact(), Exact::Label(Surface::Cylinder { .. }))))
            .expect("a cylinder wall");
        let on = FaceSurface::of(record, face).expect("a face surface");
        let [[u0, u1], [v0, v1]] = on.domain();
        let uv = [(u0 + u1) * 0.5, (v0 + v1) * 0.5];
        let pick = on.point(uv).expect("a pick");
        let built = solidcore_ops::surface_chart::SurfaceChart::build(record, face, pick, 1e-6).expect("a chart");
        let axes = *built.chart();
        let target = doc.strong_face_target(body, face, Vec3::new(pick[0], pick[1], pick[2])).expect("a face target");
        let saved = SurfaceRef { target, chart: SurfaceChart { origin: axes.origin, u_axis: axes.u_axis, v_axis: axes.v_axis, tolerance: 1e-6 } };
        let SurfaceAttachment::Healthy { face: found, chart } = doc.surface_attachment(&saved, &state) else { panic!("the saved chart was lost") };
        assert_eq!(found, face);
        for (x, y) in [(0.0, 0.0), (1.0, 0.5), (-2.0, 0.25)] {
            let a = chart.point(x, y).expect("a point");
            let b = built.point(x, y).expect("a point");
            assert!((a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12 && (a[2] - b[2]).abs() < 1e-12, "{a:?} vs {b:?}");
            assert_eq!(chart.region(x, y), built.region(x, y));
        }
        let mut stale = saved.clone();
        stale.target.body = "Body9".into();
        assert!(matches!(doc.surface_attachment(&stale, &state), SurfaceAttachment::Lost { .. }));
    }
}
