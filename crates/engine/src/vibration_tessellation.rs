//! Boundary tessellation and volume meshing for the finite-element preview.

use fieldcraft_mesh::{
    cdt::{CdtOptions, tetrahedralize},
    surface::{BoundarySurface, SurfaceOptions},
    tet::{Order, QualityMesh, QualityOptions, canonical::CanonicalSoup, quality_mesh},
};

use solidcraft_doc::ModelBody;

pub(super) fn build(body: &ModelBody, size: f64, assembly: bool, node_limit: usize) -> fieldcraft_mesh::Result<QualityMesh> {
    let boundary = body.mesh();
    if boundary.triangles.len() > 40_000 || boundary.positions.len() > 100_000 {
        return Err(fieldcraft_mesh::Error::Limit("the boundary is too large for the vibration preview".into()));
    }
    let positions: Vec<_> = boundary.positions.iter().map(|v| [v.x, v.y, v.z]).collect();
    let soup = CanonicalSoup::new(&positions, &boundary.triangles, &boundary.tri_face)?;
    let options = SurfaceOptions { weld_distance: solidcraft_geom::LINEAR_TOL * 2.0, ..SurfaceOptions::default() };
    let surface = BoundarySurface::new(&soup.positions, &soup.triangles, &soup.tri_face, &options)?;
    let raw = tetrahedralize(&surface, &CdtOptions { max_steiner: 10_000, max_tets: 120_000 })?;
    let options = QualityOptions {
        size: Some(size),
        min_size: size / 24.0,
        order: if assembly { Order::Linear } else { Order::Quadratic },
        max_points: node_limit,
        max_tets: 50_000,
        ..QualityOptions::default()
    };
    let mut volume = quality_mesh(&surface, &raw, Vec::new(), &options)?;
    soup.restore_tags(&mut volume);
    Ok(volume)
}
