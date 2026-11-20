//! Faceted booleans with helical tools (modelled threads).

use std::f64::consts::PI;

use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::MeshGeom;

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

/// A thread of many turns of an M8 groove: the groove's facets come in sweep order, thousands of
/// them, and the cut is one valid solid that has lost exactly the groove inside the shaft.
///
/// The groove runs a turn past each end of the shaft, so the shaft keeps whole turns of it: its
/// section's 2π∫r dA (r 3.3…4, inside the shaft) per turn. The groove is faceted at 48 steps a
/// turn: at a station it is the section itself, and between two it lies on the chords, no closer
/// in than the section scaled towards the axis by cos(π/48). So the volume removed
/// lies between that of the exact helix and of the scaled section's, each analytic. The two are
/// 1.8 % apart; the window this test held before (0.6…0.95 of the shaft's volume) was over a
/// hundred times as wide.
#[test]
fn a_long_thread_cuts_to_its_groove_volume() {
    long_thread(TURNS);
}

/// The test above at a real M8 × 60 bolt's length: 48 turns, about 19,000 groove triangles.
/// Ignored because the boolean's time grows about as the square of the turns (750 s of a 945 s
/// test on CI at this size, #14453); run it with `--ignored` when working on that.
#[test]
#[ignore = "48 turns: about 15 minutes, the boolean grows about as the square of the turns (#14453)"]
fn a_bolt_length_thread_cuts_to_its_groove_volume() {
    long_thread(48);
}

/// Turns of the shaft in `a_long_thread_cuts_to_its_groove_volume`.
const TURNS: u32 = 6;

fn long_thread(turns: u32) {
    let (r, p) = (4.0, 1.25);
    let len = p * f64::from(turns);
    let shaft = cylinder(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), r, len).unwrap();
    let k = Vec3::new(0.0, 0.0, 1.0);
    let base = Vec3::new(0.0, 0.0, -p);
    let plane = Plane { origin: base, x: Vec3::new(1.0, 0.0, 0.0), y: k };
    // The section: 0.16 tall at r 3.3, 1 tall at r 4.1 (past the shaft's wall).
    let section = [Vec2::new(3.3, -0.08), Vec2::new(4.1, -0.5), Vec2::new(4.1, 0.5), Vec2::new(3.3, 0.08)];
    let steps = 48u32;
    let groove = sweep_helix_mesh(&plane, &section, base, k, p, len / p + 2.0, steps as usize).unwrap();
    let cut = faceted_boolean(&shaft, &groove, BoolOp::Cut, p * 0.005).unwrap().unwrap();
    assert!(cut.validity().is_empty(), "{:?}", cut.validity());
    // The cut's own volume (SMLib's, of its exact faces).
    let removed = PI * r * r * len - crate::measure(&cut).unwrap().volume;
    // The section scaled by s towards the axis, inside the shaft: height h(ρ) = 0.16 + 1.05 (ρ/s − 3.3)
    // for ρ from 3.3 s to r; turns × 2π∫ρ h(ρ) dρ.
    let groove_volume = |s: f64| {
        let (a, b) = (3.3 * s, r);
        let (c0, c1) = (0.16 - 1.05 * 3.3, 1.05 / s);
        f64::from(turns) * 2.0 * PI * (c0 * (b * b - a * a) / 2.0 + c1 * (b.powi(3) - a.powi(3)) / 3.0)
    };
    let (exact, chords) = (groove_volume(1.0), groove_volume((PI / f64::from(steps)).cos()));
    // 1e-4 of slack for the facets' own triangulation (a side's quads are not quite flat).
    assert!(removed > exact * (1.0 - 1e-4) && removed < chords * (1.0 + 1e-4), "{removed} not in {exact}…{chords}");
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
