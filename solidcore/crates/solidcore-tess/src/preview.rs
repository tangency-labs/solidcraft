//! A bounded provisional display of a decoded body, before its edit mesh: the budget is taken
//! before meshing (at least one triangle per face) and charged with what the mesh used; the
//! planner bounds its own work per face, so hostile records cost a bounded amount.

use crate::{Mesh, MeshTolerance, Result, TessError};

fn over() -> TessError {
    TessError::Invalid("native B-rep: preview budget".into())
}

/// The tolerance of a preview mesh of a body of size `size` (its box diagonal), finer with
/// `detail` (1–8): a hundredth of the body over `detail`, at least 1 µm, at the display angle.
#[must_use]
pub fn tolerance(size: f64, detail: usize) -> MeshTolerance {
    // Proven: the clamp bounds are the constants 1 <= 8.
    let detail = detail.clamp(1, 8) as f64;
    MeshTolerance::display((size * 0.01 / detail).max(1e-3))
}

/// Whether a body of `faces` faces may be meshed within `remaining` triangles: each face takes
/// at least one.
///
/// # Errors
/// [`TessError::Invalid`] when it may not.
pub fn admit(faces: usize, remaining: usize) -> Result<()> {
    if faces > remaining {
        Err(over())
    } else {
        Ok(())
    }
}

/// Charge `mesh` to `remaining`: refused, and nothing charged, when it has more triangles than
/// are left or a position that is not finite.
///
/// # Errors
/// [`TessError::Invalid`] when it is refused.
pub fn charge(mesh: &Mesh, remaining: &mut usize) -> Result<()> {
    if mesh.triangles.len() > *remaining || mesh.positions.iter().any(|p| !p.is_finite()) {
        return Err(over());
    }
    *remaining -= mesh.triangles.len();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;

    fn mesh(triangles: usize, at: Vec3) -> Mesh {
        Mesh {
            positions: vec![Vec3::ZERO, Vec3::X, at],
            triangles: vec![[0, 1, 2]; triangles],
            ..Mesh::default()
        }
    }

    #[test]
    fn the_budget_is_taken_before_meshing_and_charged_after() {
        assert!(admit(6, 5).is_err());
        assert!(admit(6, 6).is_ok());
        let mut left = 10;
        assert!(charge(&mesh(11, Vec3::Y), &mut left).is_err());
        assert_eq!(left, 10);
        assert!(charge(&mesh(4, Vec3::new(0.0, f64::NAN, 0.0)), &mut left).is_err());
        assert_eq!(left, 10);
        assert!(charge(&mesh(4, Vec3::Y), &mut left).is_ok());
        assert_eq!(left, 6);
        assert_eq!(tolerance(100.0, 0), tolerance(100.0, 1));
        assert_eq!(
            tolerance(100.0, 9),
            MeshTolerance::display(100.0 * 0.01 / 8.0)
        );
        assert_eq!(tolerance(1e-3, 8).chord, 1e-3);
    }
}
