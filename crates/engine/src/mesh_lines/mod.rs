//! The lines of placed bodies and shown sketches as a view sees them: the bodies' export meshes
//! projected, their edges and silhouettes split where a nearer surface covers them. The view
//! DXF (`file.export_view_dxf`) and drawing views (W07, #5866) draw with it; each caller chooses
//! the bodies, the chord tolerance, the view, and whether the lines name what drew them.
//!
//! Curves and silhouettes come from the meshes, so they follow the surfaces to within the chord
//! tolerance, no closer.
mod meshes;
mod progress;
mod projection;
mod scene;

pub(crate) use meshes::{Ask, Scene, WORKERS, prepare};
pub(crate) use progress::Stage;
pub(crate) use projection::{Origin, Owner, Part, Path, View, clip};
pub(crate) use scene::drawing;
