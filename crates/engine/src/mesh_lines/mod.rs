//! The lines of placed bodies and shown sketches as a view sees them: the bodies' export meshes
//! projected, their edges and silhouettes split where a nearer surface covers them. The view
//! DXF (`file.export_view_dxf`) draws with it, and drawing views (W07, #5866) will too; each
//! caller chooses the bodies, the chord tolerance and the view.
//!
//! Curves and silhouettes come from the meshes, so they follow the surfaces to within the chord
//! tolerance, no closer.
mod meshes;
mod progress;
mod projection;
mod scene;

pub(crate) use meshes::{WORKERS, prepare};
pub(crate) use progress::Stage;
pub(crate) use projection::{Part, Path, View};
pub(crate) use scene::drawing;
