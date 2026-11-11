//! SMLib's sheet operations: sewing a body's faces along the edges that coincide, and a sheet's
//! offset along its faces' normals. Each returns a new body and leaves its input unchanged;
//! tagged inputs ([`Brep::tagged`]) give results whose [`Brep::history`] says where each face and
//! edge came from.

use serde_json::json;

use crate::trace;
use crate::{Brep, Result, core};

impl Brep {
    /// The faces sewn along the vertices and edges that coincide within `tolerance` mm, oriented
    /// outward where they close into a solid, else only made to agree across their shared edges;
    /// with whether the result is a closed solid.
    pub fn stitch(&self, tolerance: f64) -> Result<(Brep, bool)> {
        let r = core::merge::stitch(self.core(), tolerance).map(|(b, closed)| (Brep::made_core(b), closed));
        trace::call("stitch", &[self], &r, |(b, _)| vec![b.id], || json!({"tolerance": tolerance}));
        r
    }

    /// This sheet with every face moved onto its surface's offset by `distance` along its normal
    /// (negative: against it). A closed solid is [`crate::Error::Argument`].
    pub fn offset_sheet(&self, distance: f64) -> Result<Brep> {
        let r = core::offset::offset_sheet(self.core(), distance).map(Brep::made_core);
        trace::made("offset_sheet", &[self], &r, || json!({"distance": distance}));
        r
    }
}
