//! Bounded provisional display of a decoded body, before its edit mesh. The budget is taken
//! before meshing (at least one triangle per face) and charged with what the mesh used; the
//! planner bounds its own work per face, so hostile records cost a bounded amount.
use super::{Body, Result, invalid};
use solidcraft_geom::Mesh;

/// A coarse mesh of `body` (finer with `detail`, 1–8) charged to `remaining` triangles.
pub fn native_preview_mesh(body: &Body, detail: usize, remaining: &mut usize) -> Result<Mesh> {
    if body.mesh.is_some() {
        return Err(invalid("native preview requires B-rep"));
    }
    if body.face_count() > *remaining {
        return Err(invalid("preview budget"));
    }
    let detail = detail.clamp(1, 8) as f64;
    let chord = (body.size() * 0.01 / detail).max(1e-3);
    let mesh = body.mesh(&crate::MeshTolerance::display(chord))?;
    if mesh.triangles.len() > *remaining || mesh.positions.iter().any(|p| !p.is_finite()) {
        return Err(invalid("preview budget"));
    }
    *remaining -= mesh.triangles.len();
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;
    #[test]
    fn native_preview_is_finite_and_budgeted_before_meshing() {
        for body in [
            crate::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap(),
            crate::sphere(Vec3::ZERO, 10.).unwrap(),
            crate::torus(Vec3::ZERO, 20., 4.).unwrap(),
        ] {
            let body = crate::decode_native_body(&crate::encode_native_body(&body).unwrap()).unwrap();
            assert!(native_preview_mesh(&body, 8, &mut 0).is_err());
            let mut budget = 100_000;
            let mesh = native_preview_mesh(&body, 8, &mut budget).unwrap();
            assert!(!mesh.triangles.is_empty());
            assert!(mesh.triangles.len() <= 100_000 - budget);
            assert!(mesh.positions.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
        }
    }
}
