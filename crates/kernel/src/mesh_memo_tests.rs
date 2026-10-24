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

/// Work done once per body: a measure kept with the solid (following its split lines), and the
/// healed body and edges a blend reuses while it runs (#2341). Each gives what the work itself
/// gives.
mod once_per_body {
    use solidcraft_geom::{Plane, Vec3};

    use crate::body::SEARCHED;
    use crate::measure::MEASURED;
    use crate::memo::MADE;
    use crate::{Body, SplitTool, box_solid, chamfer, measure, split_faces, torus};

    fn measured() -> usize {
        MEASURED.with(|n| n.get())
    }

    /// A 60 × 20 × 2 plate with three 6 × 6 × 3 bosses on top, 20 mm apart.
    fn bosses() -> Body {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(60.0, 20.0, 2.0)).unwrap();
        for k in 0..3 {
            let x = 7.0 + 20.0 * k as f64;
            let boss = box_solid(Vec3::new(x, 7.0, 2.0), Vec3::new(x + 6.0, 13.0, 5.0)).unwrap();
            b = crate::boolean(&b, &boss, crate::BoolOp::Union).unwrap().unwrap();
        }
        b
    }

    /// The four top edges of boss `k`.
    fn top_edges(k: usize) -> Vec<Vec3> {
        let x = 7.0 + 20.0 * k as f64;
        vec![Vec3::new(x + 3.0, 7.0, 5.0), Vec3::new(x + 6.0, 10.0, 5.0), Vec3::new(x + 3.0, 13.0, 5.0), Vec3::new(x, 10.0, 5.0)]
    }

    #[test]
    fn a_body_is_measured_once_and_its_clones_share_the_measure() {
        let b = bosses();
        let before = measured();
        let m = measure(&b).unwrap();
        let again = measure(&b.clone()).unwrap();
        assert_eq!(measured() - before, 1);
        assert_eq!((m.volume.to_bits(), m.area.to_bits()), (again.volume.to_bits(), again.area.to_bits()));
        assert!((m.volume - (2400.0 + 3.0 * 108.0)).abs() < 1e-6, "{}", m.volume);
    }

    #[test]
    fn a_chain_of_chamfers_measures_each_body_once() {
        // Each chamfer checks its input and its result, and its result is the next one's input:
        // the plate and the three results, not six measures.
        let b = bosses();
        let before = measured();
        let mut cur = b;
        for k in 0..3 {
            cur = chamfer(&cur, &top_edges(k), 0.5).unwrap();
        }
        assert_eq!(measured() - before, 4);
        // A 6 × 6 top loses 2·L·d² − 4d³/3 to a 0.5 mm chamfer of its edges.
        let v = measure(&cur).unwrap().volume;
        assert!((v - (2400.0 + 3.0 * (108.0 - 3.0 + 1.0 / 6.0))).abs() < 1e-6, "{v}");
    }

    #[test]
    fn a_measure_follows_the_split_lines_of_the_body_measured() {
        // A box top split in two: kept apart while the split lines are on the body, merged without.
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let top = b.faces(0.01).unwrap().iter().position(|f| (f.centroid.z - 30.0).abs() < 1e-6).unwrap();
        let pl = Plane::new(Vec3::new(4.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let split = split_faces(&b, &[top], &SplitTool::Plane(pl)).unwrap();
        let kept = measure(&split).unwrap().merged.faces;
        let plain = split.clone().with_split_lines(None);
        let merged = measure(&plain).unwrap().merged.faces;
        assert_eq!((kept, merged), (7, 6));
        assert_eq!(measure(&split).unwrap().merged.faces, 7);
    }

    #[test]
    fn a_blend_heals_and_finds_edges_once_per_body() {
        let b = bosses();
        crate::memo::scope(|| {
            MADE.with(|n| n.set((0, 0)));
            let (h1, h2) = (crate::boolean_record::healed(&b).unwrap(), crate::boolean_record::healed(&b.clone()).unwrap());
            assert!(std::sync::Arc::ptr_eq(&h1.solid, &h2.solid));
            let (e1, e2) = (b.edges(0.01).unwrap(), b.edges(0.01).unwrap());
            assert_eq!(e1.len(), e2.len());
            assert!(b.nearest_edge(top_edges(0)[0], 0.01).unwrap().is_some());
            assert_eq!(MADE.with(|n| n.get()), (1, 1));
            // Other split lines are another body to heal.
            let line = |x: f64| Some(std::sync::Arc::new(vec![vec![Vec3::new(x, 0.0, 2.0), Vec3::new(x, 20.0, 2.0)]]));
            let (one, two) = (b.clone().with_split_lines(line(1.0)), b.clone().with_split_lines(line(2.0)));
            crate::boolean_record::healed(&one).unwrap();
            crate::boolean_record::healed(&one.clone()).unwrap();
            crate::boolean_record::healed(&two).unwrap();
            assert_eq!(MADE.with(|n| n.get()).0, 3);
        });
        // Outside a blend nothing is kept.
        let (h1, h2) = (crate::boolean_record::healed(&b).unwrap(), crate::boolean_record::healed(&b).unwrap());
        assert!(!std::sync::Arc::ptr_eq(&h1.solid, &h2.solid));
    }

    #[test]
    fn an_unchanged_revolved_face_is_searched_once() {
        // A torus is a revolved circle. Its faces are searched when the body is made, and the turned
        // faces once more by the next body; not again by a body made of the same faces.
        let before = SEARCHED.with(|n| n.get());
        let t = torus(Vec3::ZERO, 10.0, 3.0).unwrap();
        assert!(SEARCHED.with(|n| n.get()) > before);
        let turned = Body::new(t.deep_copy()).unwrap();
        let searched = SEARCHED.with(|n| n.get());
        let again = Body::new(turned.deep_copy()).unwrap();
        assert_eq!(SEARCHED.with(|n| n.get()), searched);
        let (a, b) = (measure(&turned).unwrap(), measure(&again).unwrap());
        assert_eq!((a.volume.to_bits(), a.area.to_bits()), (b.volume.to_bits(), b.area.to_bits()));
    }

    #[test]
    fn one_blend_heals_its_input_and_finds_its_edges_once() {
        // Without the memo the chamfer made 8 edge sets, and the fillet 2 heals and 6 edge sets.
        let b = bosses();
        MADE.with(|n| n.set((0, 0)));
        chamfer(&b, &top_edges(0), 0.5).unwrap();
        assert_eq!(MADE.with(|n| n.get()), (1, 2));
        MADE.with(|n| n.set((0, 0)));
        crate::fillet(&b, &top_edges(1), 0.5).unwrap();
        assert_eq!(MADE.with(|n| n.get()), (1, 1));
    }

    /// Point tests on one body while a blend runs mesh it once; outside a blend, each meshes it.
    #[test]
    fn a_body_is_meshed_once_for_its_point_tests_while_a_blend_runs() {
        let b = bosses();
        let probes = [Vec3::new(10.0, 10.0, 1.0), Vec3::new(10.0, 10.0, 9.0), Vec3::new(30.0, 10.0, 4.0)];
        let meshed = || crate::memo::MESHED.with(|n| n.get());
        let before = meshed();
        let inside: Vec<bool> = crate::memo::scope(|| probes.iter().map(|p| b.contains(*p)).collect());
        assert_eq!(meshed() - before, 1);
        let before = meshed();
        let outside: Vec<bool> = probes.iter().map(|p| b.clone().contains(*p)).collect();
        assert_eq!(meshed() - before, probes.len());
        assert_eq!(inside, outside);
        assert_eq!(inside, [true, false, true]);
    }
}
