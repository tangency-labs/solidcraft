//! Untrim and extend of a sheet's faces, and a body trimmed along curves dropped square onto its
//! faces. Each returns a new body and leaves its input unchanged; faces, holes and regions are
//! chosen by points, as [`crate::ops`] chooses them.

use serde_json::json;

use crate::sweeps::Curve;
use crate::trace;
use crate::{Brep, Result, core};

/// How [`Brep::extend_sheet`] continues a surface past its side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Continuation {
    /// The surface's own geometry (a B-spline's end spans continued).
    Natural,
    /// A run-off tangent to the surface across the side (G1).
    Tangent,
    /// A run-off that also keeps the surface's curvature across the side (G2).
    Curvature,
}

/// The inner loops [`Brep::untrim_face`] takes away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Holes<'a> {
    All,
    /// The inner loop nearest each point (within the tolerance, else [`crate::Error::NoMatch`]).
    At(&'a [[f64; 3]]),
}

impl Brep {
    /// The face nearest `point` (within `tolerance`) restored towards its surface: the holes
    /// `holes` names taken away and, with `outer`, its outer boundary back on the surface's
    /// parameter box (a free-form surface's own, an analytic one's the box of the face's span; one
    /// all the way round closes on the surface's seam). The face alone, or
    /// with `in_place` the whole body with it replaced. Tags follow the face and its edges.
    pub fn untrim_face(&self, point: [f64; 3], tolerance: f64, holes: Holes, outer: bool, in_place: bool) -> Result<Brep> {
        let holes = match holes {
            Holes::All => None,
            Holes::At(at) => Some(at),
        };
        let r = core::trim::untrim_face(self.core(), point, tolerance, holes, outer, in_place).map(Brep::made_core);
        trace::made(
            "untrim_face",
            &[self],
            &r,
            || json!({"face": trace::picks(self, trace::PickKind::NearestFace, &[point], tolerance), "holes": holes, "outer": outer, "in_place": in_place}),
        );
        r
    }

    /// The one-face sheet `self`, whose face is its surface's whole parameter box, continued
    /// `distance` past the side its edge nearest `point` (within `tolerance`) runs along, as
    /// `how` says: SMLib's extended surface over the grown box, the face and its sides carrying
    /// the tags of those they continue. A sheet of another shape is [`crate::Error::Unsupported`].
    pub fn extend_sheet(&self, point: [f64; 3], tolerance: f64, distance: f64, how: Continuation) -> Result<Brep> {
        let r = core::trim::extend_sheet(self.core(), point, tolerance, distance, how).map(Brep::made_core);
        trace::made("extend_sheet", &[self], &r, || {
            let how = match how {
                Continuation::Natural => "natural",
                Continuation::Tangent => "tangent",
                Continuation::Curvature => "curvature",
            };
            json!({"edge": trace::picks(self, trace::PickKind::NearestEdge, &[point], tolerance), "distance": distance, "how": how})
        });
        r
    }

    /// A point inside each face, in SMLib's face order: on the face's surface, inside its trims
    /// (`SmFace::CalculateAnInternalPoint`).
    pub fn face_points(&self) -> Result<Vec<[f64; 3]>> {
        core::trim::face_points(self.core())
    }

    /// A copy cut along `chains` (each a list of curves meeting end to end) dropped square onto its
    /// faces (each point of a curve to its foot on the face's surface, within `tolerance`), keeping
    /// the regions the faces nearest `points` lie in: solidcore's `trim::trim_by_dropped_curves`. A
    /// face a chain bounds nothing on stays whole. Tags follow each face to its pieces. A chain that
    /// divides no face, a curve that lands on no face and a point as near one region as another are
    /// [`crate::Error::Refused`] with solidcore-ops' own refusal (`CurveDoesNotCut`, `Unsupported`).
    pub fn trim_by_dropped_curves(&self, chains: &[Vec<Curve>], points: &[[f64; 3]], tolerance: f64) -> Result<Brep> {
        let dropped: Vec<Vec<solidcore_ops::sweep::Curve>> = chains.iter().map(|chain| chain.iter().map(ops_curve).collect()).collect();
        let r = core::call::ops_as("trim_by_dropped_curves", &[("brep", self.core())], trim_refusal, || {
            solidcore_ops::trim::trim_by_dropped_curves(self.core(), &dropped, points, tolerance)
        })
        .map(Brep::made_core);
        trace::made(
            "trim_by_dropped_curves",
            &[self],
            &r,
            || json!({"chains": chains.iter().map(|chain| trace::curves(chain)).collect::<Vec<_>>(), "points": points, "tolerance": tolerance}),
        );
        r
    }
}

/// A dropped-curve trim's refusals an application words itself, carried as solidcore-ops types
/// them; any other error as every call's ([`core::call::error`]).
fn trim_refusal(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> crate::Error {
    match e {
        e @ (solidcore_ops::Error::CurveDoesNotCut | solidcore_ops::Error::Unsupported(_)) => crate::Error::Refused(e),
        e => core::call::error(call, e, detail),
    }
}

/// This crate's curve as solidcore-ops' (the same lines, arcs and NURBS, field for field).
fn ops_curve(c: &Curve) -> solidcore_ops::sweep::Curve {
    use solidcore_ops::sweep::Curve as Ops;
    match c {
        Curve::Line { start, end } => Ops::Line { start: *start, end: *end },
        Curve::Arc { center, axis, reference, radius, start, end } => {
            Ops::Arc { center: *center, axis: *axis, reference: *reference, radius: *radius, start: *start, end: *end }
        }
        Curve::Nurbs { degree, points, knots } => Ops::Nurbs { degree: *degree, points: points.clone(), knots: knots.clone() },
    }
}
