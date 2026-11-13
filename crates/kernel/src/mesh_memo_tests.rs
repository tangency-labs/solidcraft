use super::*;
use crate::{BoolOp, boolean, box_solid, cylinder, measure};
use solidcraft_geom::tess::EvaluateMesh as _;

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

/// Regions cut from a body that keeps its meshes come out as from the body itself, to the bit.
/// SMLib's booleans read the exact record and mesh neither operand, so they leave the memo
/// empty; the body's own meshing then fills it once per tolerance and reuses it.
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
    assert_eq!(memo.made.lock().unwrap().len(), 0, "a boolean meshes neither operand");
    // A mesh from the memo is the mesh made afresh.
    let (a, b) = (kept.mesh(&crate::MeshTolerance::chord(0.05)).unwrap(), plate.mesh(&crate::MeshTolerance::chord(0.05)).unwrap());
    let (a2, _) = (kept.mesh(&crate::MeshTolerance::chord(0.05)).unwrap(), ());
    assert_eq!(memo.made.lock().unwrap().len(), 1, "one tolerance, one meshing");
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
    let copies: Vec<Body> =
        (0..2).map(|_| crate::transform(&part, shift, Vec3::ZERO, Vec3::Z, angle).unwrap().with_display_share(&share, t)).collect();
    let tol = crate::MeshTolerance::display_of(part.size());
    let original = part.mesh_plan(&tol).unwrap().evaluate_mesh();
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

/// The mesh path's mesh ([`Body::export_mesh`]) of a placed copy is its original's, placed: the
/// same triangles, its points the original's moved by the placement, the stats the original's,
/// and the original meshed once for every copy, as `Body::mesh` shares the display's.
#[test]
fn a_placed_copys_export_mesh_is_its_originals_moved() {
    let part = box_solid(Vec3::ZERO, Vec3::new(30.0, 20.0, 10.0)).unwrap();
    let share = DisplayShare::new(part.clone());
    let (angle, shift) = (0.7f64, Vec3::new(50.0, -20.0, 5.0));
    let (c, s) = (angle.cos(), angle.sin());
    let t = [[c, s, 0., 0.], [-s, c, 0., 0.], [0., 0., 1., 0.], [shift.x, shift.y, shift.z, 1.]];
    let copies: Vec<Body> =
        (0..2).map(|_| crate::transform(&part, shift, Vec3::ZERO, Vec3::Z, angle).unwrap().with_display_share(&share, t)).collect();
    let spec = MeshSpec::chord(part.size() * 2e-4);
    let original = part.export_mesh(&spec).unwrap();
    for copy in &copies {
        let m = copy.export_mesh(&spec).unwrap();
        assert_eq!(m.mesh.triangles, original.mesh.triangles);
        assert_eq!(m.mesh.tri_face, original.mesh.tri_face);
        assert_eq!(m.stats, original.stats);
        let placed = placed_mesh(&original.mesh, &t).unwrap();
        let far = m.mesh.positions.iter().zip(&placed.positions).map(|(p, q)| p.dist(*q)).fold(0.0, f64::max);
        assert!(far < 1e-9, "{far}");
    }
    assert_eq!(share.0.exported.lock().unwrap().len(), 1, "meshed once for both copies");
    // The display's memo is not the export's: the viewport's meshes are never these.
    assert_eq!(share.0.meshes.lock().unwrap().len(), 0);
}

/// A body that keeps its meshes keeps the mesh path's too, by spec: asked again, the same mesh
/// without meshing again; another spec meshes anew.
#[test]
fn a_body_keeping_its_meshes_keeps_its_export_meshes_by_spec() {
    let kept = box_solid(Vec3::ZERO, Vec3::new(30.0, 20.0, 10.0)).unwrap().keeping_meshes();
    let (a, b) = (MeshSpec::chord(0.01), MeshSpec { min_angle: 20.0, ..MeshSpec::chord(0.01) });
    let first = kept.export_mesh(&a).unwrap();
    let again = kept.export_mesh(&a).unwrap();
    assert_eq!(first.mesh.triangles, again.mesh.triangles);
    let memo = kept.meshes.as_ref().unwrap();
    assert_eq!(memo.exported.lock().unwrap().len(), 1, "one spec, one meshing");
    let _ = kept.export_mesh(&b).unwrap();
    assert_eq!(memo.exported.lock().unwrap().len(), 2);
    assert_eq!(memo.made.lock().unwrap().len(), 0, "no display mesh was made");
}

/// The mesh path is strict about its spec (an angle past the shaping's bound is refused, never
/// clamped), and a mesh body gives its own triangles with nothing to report.
#[test]
fn export_mesh_refuses_a_spec_out_of_range_and_gives_a_mesh_bodys_own_triangles() {
    let part = box_solid(Vec3::ZERO, Vec3::new(30.0, 20.0, 10.0)).unwrap();
    let e = part.export_mesh(&MeshSpec { min_angle: 31.0, ..MeshSpec::chord(0.01) }).unwrap_err().to_string();
    assert!(e.contains("smallest angle"), "{e}");
    let tri = crate::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z], &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]).unwrap();
    let m = tri.export_mesh(&MeshSpec::chord(0.01)).unwrap();
    assert_eq!(m.mesh.triangles.len(), 4);
    assert_eq!(m.stats, PlanStats::default());
    assert!(m.notes(&MeshSpec::chord(0.01)).is_empty());
}

/// A face left short of the angle is said to be short of it; one whose shaping a bound stopped
/// after it reached the angle is said to have reached it.
#[test]
fn notes_say_short_only_for_a_face_under_the_angle() {
    let spec = MeshSpec::chord(0.01);
    let unshaped =
        |face: usize, smallest_angle: f64| solidcore_tess::Unshaped { face, smallest_angle, limits: vec![solidcore_tess::ShapeLimit::Rounds] };
    let made =
        ExportMesh { mesh: Mesh::default(), stats: PlanStats { unshaped: vec![unshaped(3, 18.5), unshaped(7, 27.25)], ..PlanStats::default() } };
    let notes = made.notes(&spec);
    assert_eq!(notes.len(), 2);
    assert!(notes[0].starts_with("face 3: smallest angle 18.50°, short of the 25° asked"), "{}", notes[0]);
    assert!(!notes[1].contains("short"), "{}", notes[1]);
    assert!(notes[1].contains("face 7") && notes[1].contains("reached") && notes[1].contains("27.25°"), "{}", notes[1]);
}
