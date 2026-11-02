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
    assert_eq!(memo.shells.lock().unwrap().len(), 1, "one tolerance, one meshing");
    assert!(!memo.made.lock().unwrap().is_empty());
    // A mesh from the memo is the mesh made afresh.
    let (a, b) = (kept.tessellate_with(0.05, true).unwrap(), plate.tessellate_with(0.05, true).unwrap());
    let (a2, _) = (kept.tessellate_with(0.05, true).unwrap(), ());
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
    swapped.solid = other.solid.clone();
    let _ = kept.tessellate_with(0.5, true).unwrap();
    let m = swapped.tessellate_with(0.5, true).unwrap();
    assert_eq!(m.triangles.len(), other.tessellate_with(0.5, true).unwrap().triangles.len());
    assert!(swapped.boolean_meshes(0.01).is_none());
}

#[test]
fn plane_split_reuses_meshes_without_changing_the_halves_or_history() {
    let sphere = crate::sphere(Vec3::ZERO, 0.5).unwrap();
    let body = crate::transform_matrix(&sphere, [[20., 0., 0., 0.], [0., 30., 0., 0.], [0., 0., 22., 0.], [0., 0., 0., 1.]]).unwrap();
    let plane = solidcraft_geom::Plane::YZ;
    let parts = crate::split_by_plane(&body, &plane).unwrap();
    assert_eq!(parts.len(), 2);
    for (part, positive) in parts.iter().zip([true, false]) {
        let reference = crate::half_space(&body, &plane, positive).unwrap().0.unwrap();
        assert!(part.validity().is_empty());
        assert_eq!(part.face_count(), reference.face_count());
        let actual = part.tessellate_with(0.005, true).unwrap();
        let expected = reference.tessellate_with(0.005, true).unwrap();
        assert!((actual.measure().volume - expected.measure().volume).abs() < 1e-6);
        assert_eq!(actual.contains(Vec3::new(5., 0., 0.)), positive);
        assert_eq!(actual.contains(Vec3::new(-5., 0., 0.)), !positive);
        let history = part.history().unwrap();
        assert_eq!(history.faces.len(), part.face_count());
        assert_eq!(history.edges.len(), part.edge_count());
    }
}
