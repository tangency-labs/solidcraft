//! Faceted booleans with helical tools (modelled threads).

use std::f64::consts::PI;

use solidcraft_geom::{Plane, Vec2, Vec3};

use crate::{BoolOp, cylinder, faceted_boolean, sweep_helix_mesh};

/// A trapezoid groove screwed into a shaft: the result is closed, and the volume removed is
/// the section's 2π∫r dA per turn over the turns inside the shaft.
#[test]
fn faceted_cut_of_a_helical_groove() {
    let shaft = cylinder(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 1.5, 4.0).unwrap();
    let (p, k) = (0.5, Vec3::new(0.0, 0.0, 1.0));
    // From a turn under the shaft to its middle: 4.5 turns, the first half-turn in the air.
    let base = Vec3::new(0.0, 0.0, -p);
    let plane = Plane { origin: base, x: Vec3::new(1.0, 0.0, 0.0), y: k };
    let section = [Vec2::new(1.2, -0.06), Vec2::new(1.6, -0.06), Vec2::new(1.6, 0.06), Vec2::new(1.2, 0.06)];
    let groove = sweep_helix_mesh(&plane, &section, base, k, p, 5.0, 48).unwrap();
    let cut = faceted_boolean(&shaft, &groove, BoolOp::Cut, 1e-3).unwrap().unwrap();
    // The cut's own volume (SMLib's, of its exact faces): a mesh's at 0.01 mm sags inside the
    // shaft's round wall by 0.18 mm³, five times the 3 % the groove is held to.
    let m = crate::measure(&cut).unwrap();
    // In the shaft from z = 0: the section (r 1.2…1.5, 0.12 tall) for 4 turns, plus the part of
    // the first turn's section above z = 0 (its z range is ±0.06 about the turn's height).
    let per_turn = 2.0 * PI * 0.12 * (1.5f64.powi(2) - 1.2f64.powi(2)) / 2.0;
    let removed = PI * 2.25 * 4.0 - m.volume;
    assert!((removed - 4.0 * per_turn).abs() < 0.03 * per_turn * 4.0, "{removed} vs {}", 4.0 * per_turn);
}

/// A groove cut into the wall of a bore through a block (a nut): it runs out of both ends.
#[test]
fn faceted_cut_into_a_bore() {
    let nut = crate::box_solid(Vec3::new(-3.0, -3.0, 0.0), Vec3::new(3.0, 3.0, 2.0)).unwrap();
    let bore = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 0.0, 1.0), 1.23, 4.0).unwrap();
    let nut = crate::boolean(&nut, &bore, BoolOp::Cut).unwrap().unwrap();
    let v0 = nut.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
    let (p, k) = (0.5, Vec3::new(0.0, 0.0, 1.0));
    let base = Vec3::new(0.0, 0.0, -p);
    let plane = Plane { origin: base, x: Vec3::new(1.0, 0.0, 0.0), y: k };
    let section = [Vec2::new(1.1, -0.1), Vec2::new(1.5, -0.1), Vec2::new(1.5, 0.1), Vec2::new(1.1, 0.1)];
    let groove = sweep_helix_mesh(&plane, &section, base, k, p, 6.0, 48).unwrap();
    let cut = faceted_boolean(&nut, &groove, BoolOp::Cut, 2.5e-3).unwrap().unwrap();
    let removed = v0 - cut.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
    // Four turns of the section's part outside the bore (r 1.23…1.5, 0.2 tall).
    let want = 4.0 * PI * 0.2 * (1.5f64.powi(2) - 1.23f64.powi(2));
    assert!((removed - want).abs() < 0.03 * want, "{removed} vs {want}");
}

/// #1250: a long thread (48 turns of an M8 groove on a 60 mm shaft). The groove's facets come in
/// sweep order; taken as BSP splitters in that order they made the tree as deep as the facet
/// list ("planar boolean: tree too deep", after minutes and gigabytes). It cuts now, closed.
#[test]
fn a_long_thread_cuts_without_a_deep_tree() {
    let (r, len, p) = (4.0, 60.0, 1.25);
    let shaft = cylinder(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), r, len).unwrap();
    let k = Vec3::new(0.0, 0.0, 1.0);
    let base = Vec3::new(0.0, 0.0, -p);
    let plane = Plane { origin: base, x: Vec3::new(1.0, 0.0, 0.0), y: k };
    let section = [Vec2::new(3.3, -0.08), Vec2::new(4.1, -0.5), Vec2::new(4.1, 0.5), Vec2::new(3.3, 0.08)];
    let groove = sweep_helix_mesh(&plane, &section, base, k, p, len / p + 2.0, 48).unwrap();
    let cut = faceted_boolean(&shaft, &groove, BoolOp::Cut, p * 0.005).unwrap().unwrap();
    let v = cut.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().measure().volume;
    let v0 = PI * r * r * len;
    assert!(v < v0 * 0.95 && v > v0 * 0.6, "{v} of {v0}");
}

/// What lies past each end of a shaft's face: the d16 collar under it, the air over its cap.
#[test]
fn material_past_the_ends_of_a_shaft_face() {
    let k = Vec3::new(0.0, 0.0, 1.0);
    let collar = cylinder(Vec3::ZERO, k, 8.0, 10.0).unwrap();
    let shaft = cylinder(Vec3::new(0.0, 0.0, 10.0), k, 5.0, 25.0).unwrap();
    let part = crate::boolean(&collar, &shaft, BoolOp::Union).unwrap().unwrap();
    let c = crate::cylinder_face_at(&part, Vec3::new(5.0, 0.0, 20.0)).unwrap();
    assert!((c.radius - 5.0).abs() < 1e-9 && c.start < c.end, "{c:?}");
    let at = |z: f64| (z - c.axis_point.z) / c.axis.z;
    let (lo, hi) = (at(10.0).min(at(35.0)), at(10.0).max(at(35.0)));
    assert!((lo - c.start).abs() < 1e-6 && (hi - c.end).abs() < 1e-6, "{c:?}");
    let below = if c.axis.z > 0.0 { (c.start - 2.25, c.start - 1.5) } else { (c.end + 1.5, c.end + 2.25) };
    let above = if c.axis.z > 0.0 { (c.end + 1.5, c.end + 2.25) } else { (c.start - 2.25, c.start - 1.5) };
    assert!(crate::material_in_ring(&part, &c, 4.19, 5.0, below).unwrap(), "the collar is past the shaft's bottom end");
    assert!(!crate::material_in_ring(&part, &c, 4.19, 5.0, above).unwrap(), "nothing is past its top end");
    // Along the face itself the ring is the shaft's own material.
    assert!(crate::material_in_ring(&part, &c, 4.19, 4.99, (lo + 5.0, lo + 6.0)).unwrap());
}
