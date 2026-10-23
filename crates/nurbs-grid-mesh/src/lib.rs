//! # nurbs-grid-mesh
//!
//! A trimmed NURBS face meshed as a grid in its own (u, v). Built for dense surfaces, control
//! nets in the hundreds each way, where a general mesher's refinement tree spends seconds per
//! face: on a degree-7 surface of 318 x 281 poles this meshes to 0.312 mm in about 0.12 s,
//! where SMLib takes 12 s and leaves a fifth of its triangles over that tolerance. The README
//! has that measurement and twelve more surfaces that are not drawn on a grid.
//!
//! ## The method
//!
//! 1. **Grid lines** in u and in v, as many in each knot span as that span's curvature needs:
//!    the second derivative of a B-spline is bounded by its second-derivative control points, and
//!    a chord of parameter length h sags at most h^2 |C''| / 8 ([`grid_lines`]).
//! 2. **Measured refinement**: every cell's edge midpoints and both diagonal midpoints are
//!    compared with the surface at the same (u, v), and any column or row past
//!    [`MeshOptions::measured_fraction`] of the tolerance is split until none is. The bound in
//!    step 1 is only an estimate for rational surfaces and ignores the twist a diagonal sees.
//! 3. **The boundary** loops, given in (u, v) with their 3D points, are split wherever they cross
//!    a grid line. A cell they do not touch is wholly in or wholly out, decided once per row by
//!    crossings, and becomes two triangles. A cell they do touch is cut along them: the pieces of
//!    loop inside the cell, joined by the cell's own corners going anticlockwise, are the face's
//!    polygons in that cell, and those are ear-clipped ([`mesh_trimmed`]).
//! 4. **Points**: inside, the surface's, evaluated a grid row at a time (basis functions once per
//!    line, the sum over u contracted into one row of poles, then a short dot product per point);
//!    on the boundary, the loop's own 3D points, with crossings interpolated along the loop's 3D
//!    segment. The face ends exactly where its edge polylines are, so a neighbor meshed from the
//!    same polylines meets it.
//!
//! ## What the tolerance means
//!
//! Every cell's edge and diagonal midpoints lie within 3/4 of the tolerance of the surface (by
//! default). For a locally quadratic surface the error inside a triangle is at most 4/3 of the
//! worst of those, so the triangle stays within the tolerance. That is measured at the places
//! the error peaks, not proved; [`deviation`] measures any mesh against its surface, so none of
//! it has to be taken on trust.
//!
//! Library code never panics, unwraps or indexes: bad input comes back as a [`MeshError`].
#![forbid(unsafe_code)]

mod deviation;
mod error;
mod grid;
mod surface;
mod trim;

pub use deviation::{deviation, Deviation};
pub use error::MeshError;
pub use grid::{grid_lines, Dir, GridLines};
pub use surface::{NurbsSurface, SurfacePoint};
pub use trim::{mesh_trimmed, LoopPoint, MeshOptions, TrimmedMesh};
