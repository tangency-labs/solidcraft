use super::*;
use crate::{BoolOp, boolean, box_solid, cylinder, measure};

/// A plate with a row of holes and a rounded rim along one edge (so that it is no prism the 2D
/// route would take), as a large body many small regions are cut from.
fn plate() -> Body {
    let rim = cylinder(Vec3::new(0.0, 0.0, 3.0), Vec3::X, 4.0, 300.0).unwrap();
    let mut plate = box_solid(Vec3::ZERO, Vec3::new(300.0, 200.0, 3.0)).unwrap();
    plate = boolean(&plate, &rim, BoolOp::Union).unwrap().unwrap();
    for k in 0..12 {
        let hole = cylinder(Vec3::new(20.0 + 22.0 * k as f64, 100.0, -1.0), Vec3::Z, 2.6, 5.0).unwrap();
        plate = boolean(&plate, &hole, BoolOp::Cut).unwrap().unwrap();
    }
    plate
}

/// Regions cut from a body that keeps its meshes come out as from the body itself, to the bit,
/// and the body is meshed once for all of them.
#[test]
fn regions_cut_from_a_body_keeping_its_meshes_are_unchanged() {
    let plate = plate();
    let kept = plate.keeping_meshes();
    for k in [0.0, 3.0, 7.0] {
        let x = 20.0 + 22.0 * k;
        let region = box_solid(Vec3::new(x - 8.0, 92.0, -2.0), Vec3::new(x + 8.0, 108.0, 5.0)).unwrap();
        let want = boolean(&plate, &region, BoolOp::Intersect).unwrap().unwrap();
        let got = boolean(&kept, &region, BoolOp::Intersect).unwrap().unwrap();
        let (w, g) = (measure(&want).unwrap(), measure(&got).unwrap());
        assert_eq!(got.face_count(), want.face_count(), "region at {x}");
        assert_eq!(g.volume.to_bits(), w.volume.to_bits(), "region at {x}");
        let cut = boolean(&kept, &region, BoolOp::Cut).unwrap().unwrap();
        assert_eq!(
            measure(&cut).unwrap().volume.to_bits(),
            measure(&boolean(&plate, &region, BoolOp::Cut).unwrap().unwrap()).unwrap().volume.to_bits()
        );
    }
    let memo = kept.meshes.as_ref().unwrap();
    assert_eq!(memo.made.lock().unwrap().len(), 1, "one tolerance, one meshing");
    // A mesh from the memo is the mesh made afresh.
    let (a, b) = (kept.mesh(&crate::MeshTolerance::chord(0.05)).unwrap(), plate.mesh(&crate::MeshTolerance::chord(0.05)).unwrap());
    let (a2, _) = (kept.mesh(&crate::MeshTolerance::chord(0.05)).unwrap(), ());
    for m in [&a, &a2] {
        assert_eq!(m.triangles, b.triangles);
        assert!(
            m.positions
                .iter()
                .zip(&b.positions)
                .all(|(p, q)| p.x.to_bits() == q.x.to_bits() && p.y.to_bits() == q.y.to_bits() && p.z.to_bits() == q.z.to_bits())
        );
    }
}

/// A copy whose solid was replaced does not take meshes of the solid it replaced.
#[test]
fn a_copy_with_another_solid_makes_its_own_meshes() {
    let kept = plate().keeping_meshes();
    let other = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let mut swapped = kept.clone();
    swapped.record = other.record.clone();
    swapped.smlib = other.smlib.clone();
    let _ = kept.mesh(&crate::MeshTolerance::chord(0.5)).unwrap();
    let m = swapped.mesh(&crate::MeshTolerance::chord(0.5)).unwrap();
    assert_eq!(m.triangles.len(), other.mesh(&crate::MeshTolerance::chord(0.5)).unwrap().triangles.len());
}

/// A placed copy that shows its original's display is planned as the original, moved: the same
/// triangles on every face, its points the original's placed (so within the original's chord of
/// the copy's surfaces), and the original planned once for every copy.
#[test]
fn a_placed_copy_is_planned_as_its_original_moved() {
    let part = plate();
    let share = DisplayShare::new(part.clone());
    let (angle, shift) = (0.7f64, Vec3::new(50.0, -20.0, 5.0));
    let (c, s) = (angle.cos(), angle.sin());
    let t = [[c, s, 0., 0.], [-s, c, 0., 0.], [0., 0., 1., 0.], [shift.x, shift.y, shift.z, 1.]];
    let copies: Vec<Body> = (0..2).map(|_| crate::transform(&part, shift, Vec3::ZERO, Vec3::Z, angle).unwrap().with_display_share(&share, t)).collect();
    let tol = crate::MeshTolerance::display_of(part.size());
    let original = part.mesh_plan(&tol).unwrap().evaluate();
    for copy in &copies {
        let m = copy.mesh_of(&copy.mesh_plan(&tol).unwrap()).unwrap();
        assert_eq!(m.triangles, original.triangles);
        assert_eq!(m.tri_face, original.tri_face);
        let placed = placed_mesh(&original, &t).unwrap();
        let far = m.positions.iter().zip(&placed.positions).map(|(p, q)| p.dist(*q)).fold(0.0, f64::max);
        assert!(far < 1e-9, "{far}");
    }
    assert_eq!(share.0.plans.lock().unwrap().len(), 1, "planned once for both copies");
}
