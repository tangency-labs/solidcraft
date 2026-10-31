//! Untrim and extend of a sheet's faces. Each returns a new body and leaves its input unchanged;
//! faces and holes are chosen by points, as [`crate::ops`] chooses them.

use crate::{Brep, Result, core, error, ffi, init, route};

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
        if route::Op::Split.backend() == route::Backend::Solidcore {
            return core::trim::untrim_face(self.core()?, point, tolerance, holes, outer, in_place).map(Brep::made_core);
        }
        init()?;
        ffi::trim::untrim_face(self.cpp()?, &point, tolerance, holes, outer, in_place).map(Brep::made).map_err(|s| match s {
            ffi::ops::ERR_PICK_NONE => crate::Error::NoMatch { point: 0 },
            s => error(s),
        })
    }

    /// The one-face sheet `self`, whose face is its surface's whole parameter box, continued
    /// `distance` past the side its edge nearest `point` (within `tolerance`) runs along, as
    /// `how` says: SMLib's extended surface over the grown box, the face and its sides carrying
    /// the tags of those they continue. A sheet of another shape is [`crate::Error::Unsupported`].
    pub fn extend_sheet(&self, point: [f64; 3], tolerance: f64, distance: f64, how: Continuation) -> Result<Brep> {
        if route::Op::Split.backend() == route::Backend::Solidcore {
            return core::trim::extend_sheet(self.core()?, point, tolerance, distance, how).map(Brep::made_core);
        }
        init()?;
        let mode = match how {
            Continuation::Natural => ffi::trim::EXTEND_NATURAL,
            Continuation::Tangent => ffi::trim::EXTEND_TANGENT,
            Continuation::Curvature => ffi::trim::EXTEND_CURVATURE,
        };
        ffi::trim::extend_sheet(self.cpp()?, &point, tolerance, distance, mode).map(Brep::made).map_err(|s| match s {
            ffi::ops::ERR_PICK_NONE => crate::Error::NoMatch { point: 0 },
            s => error(s),
        })
    }

    /// A point inside each face, in SMLib's face order: on the face's surface, inside its trims
    /// (`SmFace::CalculateAnInternalPoint`).
    pub fn face_points(&self) -> Result<Vec<[f64; 3]>> {
        if route::Op::Split.backend() == route::Backend::Solidcore {
            return core::trim::face_points(self.core()?);
        }
        init()?;
        let [faces, _, _] = self.counts()?;
        ffi::trim::face_points(self.cpp()?, faces).map_err(error)
    }
}
