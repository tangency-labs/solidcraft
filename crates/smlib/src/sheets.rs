//! SMLib's sheet operations: sewing a body's faces along the edges that coincide, and a sheet's
//! offset along its faces' normals. Each returns a new body and leaves its input unchanged;
//! tagged inputs ([`Brep::tagged`]) give results whose [`Brep::history`] says where each face and
//! edge came from.

use crate::{Brep, Result, error, ffi, init};

impl Brep {
    /// The faces sewn along the vertices and edges that coincide within `tolerance` mm, oriented
    /// outward where they close into a solid, else only made to agree across their shared edges;
    /// with whether the result is a closed solid.
    pub fn stitch(&self, tolerance: f64) -> Result<(Brep, bool)> {
        init()?;
        ffi::sheets::stitch(&self.0, tolerance).map(|(h, closed)| (Brep(h), closed)).map_err(error)
    }

    /// This sheet with every face moved onto its surface's offset by `distance` along its normal
    /// (negative: against it). A closed solid is [`crate::Error::Argument`].
    pub fn offset_sheet(&self, distance: f64) -> Result<Brep> {
        init()?;
        ffi::sheets::offset_sheet(&self.0, distance).map(Brep).map_err(error)
    }
}
