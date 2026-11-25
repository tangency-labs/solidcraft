//! Record indices to the kernel's entities, through the conversion's [`Order`]: a pick names a
//! face, edge or vertex by its index in the body's record, and [`Order`] says which kernel
//! entity that is.

use solidcore::topology::{EdgeId, FaceId, VertexId};
pub use solidcore_convert::brep::Order;

use crate::{Error, Result};

/// Record face `i`'s kernel face; [`Error::Argument`] for an index past the faces.
pub(crate) fn face(order: &Order, i: usize) -> Result<FaceId> {
    order.faces.get(i).copied().ok_or(Error::Argument)
}

/// Record edge `i`'s kernel edge; [`Error::Argument`] for an index past the edges.
pub(crate) fn edge(order: &Order, i: usize) -> Result<EdgeId> {
    order.edges.get(i).copied().ok_or(Error::Argument)
}

/// Record vertex `i`'s kernel vertex; [`Error::Argument`] for an index past the vertices.
pub(crate) fn vertex(order: &Order, i: usize) -> Result<VertexId> {
    order.vertices.get(i).copied().ok_or(Error::Argument)
}
