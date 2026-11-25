//! The lines of placed bodies and shown sketches as a view sees them: the bodies' export meshes
//! projected, their edges and silhouettes split where a nearer surface covers them. The view
//! DXF (`file.export_view_dxf`) and drawing views (W07, #5866) draw with it; each caller chooses
//! the bodies, the chord tolerance, the view, and whether the lines name what drew them.
//!
//! Curves and silhouettes come from the meshes, so they follow the surfaces to within the chord
//! tolerance, no closer.
//!
//! When the lines can't be made, [`Refused`] says what happened, and each caller adds what to do
//! about it in its own terms: a DXF's tolerance and camera, or the parts shown by a drawing view.
mod coincide;
mod meshes;
mod progress;
mod projection;
mod scene;

pub(crate) use coincide::sketch_off_lines;
pub(crate) use meshes::Ask;
pub(crate) use meshes::Placed;
pub(crate) use meshes::Scene;
pub(crate) use meshes::WORKERS;
pub(crate) use meshes::drawn_edges;
pub(crate) use meshes::placed;
pub(crate) use meshes::prepare;
pub(crate) use progress::Stage;
pub(crate) use projection::Origin;
pub(crate) use projection::Owner;
pub(crate) use projection::Part;
pub(crate) use projection::Path;
pub(crate) use projection::View;
pub(crate) use projection::clip;
pub(crate) use scene::drawing;

/// Why a view's lines weren't made, as what happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Refused {
    #[error("cancelled")]
    Cancelled,
    /// The occlusion work, or the lines and segments it makes, past their limits.
    #[error("the lines take more work than the limit allows")]
    Work,
    #[error("the mesh buffers exceed the {0} MiB limit")]
    MeshBuffers(usize),
    #[error("the meshes contain more than {0} triangles before placement")]
    MeshTriangles(usize),
    #[error("the projected mesh index exceeds the {0} MiB limit")]
    ViewNodes(usize),
    #[error("the shown sketches contain more than {0} sampled points")]
    SketchPoints(usize),
    /// Part of the model is behind a perspective camera's eye.
    #[error("the model crosses the camera's eye plane")]
    EyePlane,
    /// Numbers no view can hold, or the module's own data broken.
    #[error("{0}")]
    Invalid(&'static str),
}

impl From<&'static str> for Refused {
    fn from(what: &'static str) -> Self {
        Refused::Invalid(what)
    }
}
