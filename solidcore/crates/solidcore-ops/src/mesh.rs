//! The mesh reconstruction operation at the record boundary. The kernel fits and certifies
//! the geometry. This call converts positions on the way in and the resulting solid on the way out.

use solidcore::mesh_reconstruction::{reconstruct, MAX_TRIANGLES};
use solidcore_convert::vec3;
use solidcore_record::{brep::Brep, Vec3};

use crate::body::Body;
use crate::session::run;
use crate::{Error, Result};
use solidcore::SolidError;

/// Fit an analytic solid to a triangle mesh within the length tolerance. Returns None when
/// the mesh is not a supported analytic shape. Vertices and triangle indices come from the
/// mesh source and are independent of its display normals or tessellation settings.
///
/// # Errors
/// Malformed source meshes, invalid tolerances and failures during reconstruction or record export.
pub fn reconstruct_mesh(
    positions: &[Vec3],
    triangles: &[[u32; 3]],
    tolerance: f64,
) -> Result<Option<Brep>> {
    if triangles.len() > MAX_TRIANGLES || positions.len() > MAX_TRIANGLES * 3 {
        return Err(Error::Mesh("mesh exceeds the reconstruction budget"));
    }
    let native = positions.iter().copied().map(vec3).collect::<Vec<_>>();
    let body = run("reconstruct mesh", |k| {
        match reconstruct(k, &native, triangles, tolerance) {
            Err(SolidError::InvalidInput {
                operation: "reconstruct mesh",
                reason,
            }) => Ok(Err(Error::Mesh(reason))),
            answer => answer.map(Ok),
        }
    })??;
    body.map(|b| Body::new(b).to_record().map(|(record, _)| record))
        .transpose()
}
