//! The mesh reconstruction operation at the record boundary. The kernel fits and certifies
//! the geometry. This call converts positions on the way in and the resulting solid on the way out.

use solidcore::mesh_reconstruction::{reconstruct, MAX_TRIANGLES};
pub use solidcore::mesh_reconstruction::{ReconstructionKind, FACE_ORDER_VERSION};
use solidcore_convert::vec3;
use solidcore_record::{brep::Brep, Vec3};

use crate::body::Body;
use crate::session::run;
use crate::{Error, Result};
use solidcore::SolidError;

/// A converted source mesh at the record boundary.
#[derive(Clone, Debug)]
pub struct MeshReconstruction {
    /// The reconstructed solid as a geometry record.
    pub body: Brep,
    /// The representation certified by the kernel.
    pub kind: ReconstructionKind,
}

/// Convert a source triangle mesh within the length tolerance, retaining its facets when
/// analytic reconstruction is inapplicable. Source normals and display settings are unused.
///
/// # Errors
/// Malformed source meshes, invalid tolerances and failures during construction or record export.
pub fn reconstruct_mesh(
    positions: &[Vec3],
    triangles: &[[u32; 3]],
    tolerance: f64,
) -> Result<MeshReconstruction> {
    if triangles.len() > MAX_TRIANGLES {
        return Err(Error::Mesh(
            format!(
                "the mesh has {} triangles, more than the {MAX_TRIANGLES} conversion can take",
                triangles.len()
            )
            .into(),
        ));
    }
    let max_vertices = MAX_TRIANGLES * 3;
    if positions.len() > max_vertices {
        return Err(Error::Mesh(
            format!(
                "the mesh has {} vertices, more than the {max_vertices} conversion can take",
                positions.len()
            )
            .into(),
        ));
    }
    let native = positions.iter().copied().map(vec3).collect::<Vec<_>>();
    let body = run("reconstruct mesh", |k| {
        match reconstruct(k, &native, triangles, tolerance) {
            Err(SolidError::InvalidInput {
                operation: "reconstruct mesh",
                reason,
            }) => Ok(Err(Error::Mesh(reason.into()))),
            answer => answer.map(Ok),
        }
    })??;
    let kind = body.kind;
    let (body, _) = Body::new(body.body).to_record()?;
    Ok(MeshReconstruction { body, kind })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_meshes_export_analytic_and_faceted_records() {
        let positions = [
            [0., 0., 0.],
            [2., 0., 0.],
            [2., 3., 0.],
            [0., 3., 0.],
            [0., 0., 4.],
            [2., 0., 4.],
            [2., 3., 4.],
            [0., 3., 4.],
        ]
        .map(|[x, y, z]| Vec3::new(x, y, z));
        let triangles = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        let analytic = reconstruct_mesh(&positions, &triangles, 0.05).unwrap();
        assert_eq!(analytic.kind, ReconstructionKind::Analytic);
        assert_eq!(analytic.body.faces.len(), 6);
        analytic.body.validate_built().unwrap();
        let (body, _) = Body::from_record(&analytic.body, true).unwrap();
        assert!((body.mass_properties(1e-7).unwrap().volume - 24.).abs() < 1e-7);
        let points = [
            [1., 0., 0.],
            [0., 1., 0.],
            [-1., 0., 0.],
            [0., -1., 0.],
            [0., 0., 1.],
            [0., 0., -1.],
        ]
        .map(|[x, y, z]| Vec3::new(x, y, z));
        let triangles = [
            [4, 0, 1],
            [4, 1, 2],
            [4, 2, 3],
            [4, 3, 0],
            [5, 1, 0],
            [5, 2, 1],
            [5, 3, 2],
            [5, 0, 3],
        ];
        let faceted = reconstruct_mesh(&points, &triangles, 0.05).unwrap();
        assert_eq!(faceted.kind, ReconstructionKind::Faceted);
        assert_eq!(faceted.body.faces.len(), 8);
        faceted.body.validate_built().unwrap();
        assert!(
            (crate::faceted::faceted_volume(&faceted.body)
                .unwrap()
                .unwrap()
                - 4. / 3.)
                .abs()
                < 1e-10
        );
        let (body, _) = Body::from_record(&faceted.body, true).unwrap();
        assert!((body.mass_properties(1e-7).unwrap().volume - 4. / 3.).abs() < 1e-7);
    }

    #[test]
    fn public_mesh_conversion_preserves_malformed_and_original_budget_refusals() {
        let positions = [Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)];
        assert!(matches!(
            reconstruct_mesh(&positions, &[[0, 1, 2]], 0.05),
            Err(Error::Mesh(_))
        ));
        assert!(matches!(
            reconstruct_mesh(&positions, &[[0, 1, u32::MAX]], 0.05),
            Err(Error::Mesh(_))
        ));
        let triangles = reconstruct_mesh(&[], &vec![[0, 0, 0]; MAX_TRIANGLES + 1], 0.05)
            .unwrap_err()
            .to_string();
        assert!(
            triangles.contains("50001 triangles") && triangles.contains("50000"),
            "{triangles}"
        );
        let vertices = reconstruct_mesh(&vec![Vec3::ZERO; MAX_TRIANGLES * 3 + 1], &[], 0.05)
            .unwrap_err()
            .to_string();
        assert!(
            vertices.contains("150001 vertices") && vertices.contains("150000"),
            "{vertices}"
        );
        for tolerance in [f64::NAN, f64::INFINITY, 0., -1., 2.] {
            assert!(matches!(
                reconstruct_mesh(&positions, &[[0, 1, 2]], tolerance),
                Err(Error::Mesh(_))
            ));
        }
    }
}
