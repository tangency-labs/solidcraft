//! A join whose tool ends at other levels than the part, both prisms along that direction: a
//! hinge barrel under a plate's back edge, its ends flush with the sides of the leaf it hangs
//! from, the plate running on past them (#827 without its fillets). The barrel's ends lie in the
//! leaf's sides, which the B-rep boolean can't intersect, and there is no room to push them
//! apart (the plate goes on beyond them). The prisms are joined in 2D, exactly: the barrel is no
//! longer nudged (moved and turned by a hair) for the boolean to go through.

use std::f64::consts::PI;

use solidcraft_geom::Vec3;

use crate::*;

/// A 40 × 20 × 6 plate (z 5 … 11) with a 14 × 10 × 6 leaf under its back edge, 1 mm into it:
/// 5500 mm³.
fn plate_with_leaf() -> Body {
    let plate = box_solid(Vec3::new(0., 0., 5.), Vec3::new(40., 20., 11.)).unwrap();
    let leaf = box_solid(Vec3::new(10., 10., 0.), Vec3::new(24., 20., 6.)).unwrap();
    boolean(&plate, &leaf, BoolOp::Union).unwrap().unwrap()
}

/// Barrels along x with their axis on the leaf's back bottom edge (y 20, z 0): radius 5 (its top
/// meets the plate's back edge) and 4.9, ends flush with both of the leaf's sides, with one, and
/// running on past both under the plate. A quarter of the barrel lies in the leaf (where the
/// leaf is). Each join is exact: one solid, and every vertex on a level along x of the
/// plate, the leaf or the barrel.
#[test]
fn a_barrel_between_the_sides_of_a_leaf_joins_exactly() {
    let part = plate_with_leaf();
    for (r, x0, x1) in [(5., 10., 24.), (4.9, 10., 24.), (5., 10., 23.), (5., 8.5, 25.5)] {
        let what = format!("radius {r}, x {x0} … {x1}");
        let barrel = cylinder(Vec3::new(x0, 20., 0.), Vec3::X, r, x1 - x0).unwrap();
        let joined = boolean(&part, &barrel, BoolOp::Union).unwrap().unwrap();
        assert_eq!(joined.shell_count(), 1, "{what}");
        assert!(joined.validity().is_empty(), "{what}: {:?}", joined.validity());
        let inside = (x1.min(24.) - x0.max(10.)) / 4.;
        let want = 5500. + PI * r * r * (x1 - x0 - inside);
        let v = joined.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
        assert!((v - want).abs() < want * 1e-5, "{what}: volume {v} vs {want}");
        for p in joined.record().vertices.iter().map(|v| Vec3::from(v.point)) {
            assert!([0., 10., 24., 40., x0, x1].iter().any(|x| (p.x - x).abs() < 1e-9), "{what}: a vertex at {p:?}");
        }
    }
}
