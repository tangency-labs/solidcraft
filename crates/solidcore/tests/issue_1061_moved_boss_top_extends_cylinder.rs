//! tangency-labs/solidcore#1061: a local extended offset (`SmBrep::LocalOperation`, which
//! the fork's capi `smlib_offset_faces` reaches through its `SmOffsetFacesExecutive` for a
//! face with no blend on it) that moves a boss's top face up leaves the moved part short of
//! its closed-form volume. The faces stay exact (planes and the wall's cylinder); the
//! shortfall is the moved top's rim, the plane z = 22 meeting the cylinder, which
//! `MateEdges` marches as a B-spline 7.5e-7 off the circle (its trim on the wall a
//! degree 3 curve of 358 control points), so the top's disc loses a sliver: 9.7e-9 of the
//! volume. With `offset_analytic_mate_edges` (WR-495, the fork's tangency-labs/smlib@fddc366
//! and @98ab1b0) the rim is the exact circle and the volume is exact. WR-532
//! (`offset_extends_cones_as_themselves`) changes neither.
//!
//! The part: a 60 x 40 x 10 plate with an r10 boss on it at (30, 20), its top at z = 20,
//! moved to z = 22 by its own plane offset by 2 (`CreateOffsetSurface`, as the capi moves a
//! face): the plate plus a cylinder r10 from z = 10 to 22.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, Notifications};
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::{Brep, Kernel};

/// The volume at the integration tolerance 1e-9 (the unmoved part comes out 1.3e-14 off).
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The largest distance of the moved top's rim (the edge on the plane z = 22 and the
/// cylinder r10 about (30, 20)) from that circle, over 65 points.
fn rim_off_circle(k: &Kernel, b: &Brep) -> f64 {
    let mut rims = 0;
    let mut off: f64 = 0.0;
    for (_, rec) in b.edges().iter() {
        let Some(c) = rec.curve.as_ref() else {
            continue;
        };
        let pts: Vec<Point3> = (0..=64)
            .map(|i| {
                c.evaluate_point(k, rec.interval.evaluate(f64::from(i) / 64.0))
                    .unwrap()
            })
            .collect();
        let radius = |p: &Point3| (p.x - 30.0).hypot(p.y - 20.0);
        if pts
            .iter()
            .all(|p| (p.z - 22.0).abs() < 1e-3 && (radius(p) - 10.0).abs() < 1e-3)
        {
            rims += 1;
            for p in &pts {
                off = off.max((p.z - 22.0).abs()).max((radius(p) - 10.0).abs());
            }
        }
    }
    assert_eq!(rims, 1, "the moved top's rim");
    off
}

/// The moved part's volume relative to its closed form, `(got - want) / want`, and its
/// rim's distance from the circle.
fn moved_top(exact_mates: bool) -> (f64, f64) {
    let mut k = Kernel::default();
    k.fixes.offset_analytic_mate_edges = exact_mates;
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let boss = create_cylinder(&mut k, Point3::new(30.0, 20.0, 5.0), 10.0, 15.0).unwrap();
    let mut part = boolean_union(&mut k, plate, boss).unwrap();
    let top = planar_faces_at(&k, &part, 2, 20.0);
    assert_eq!(top.len(), 1, "the boss's top face");
    // the top's own plane offset 2 along its outward (+z) normal
    let face = part.face(top[0]).unwrap();
    let s = face.surface.as_ref().unwrap();
    let n = s
        .evaluate_normal(&k, face.uv_domain.mid(), Side::Left, Side::Left)
        .unwrap();
    let d = if n.z > 0.0 { 2.0 } else { -2.0 };
    let moved = s.create_offset_surface(&k, d, 1e-6).unwrap().unwrap();
    let mut notifications = Notifications::new(&mut part);
    local_operation(
        &mut k,
        &mut part,
        &top,
        vec![moved],
        &mut notifications.context(),
    )
    .unwrap();
    solidcore::topology::integrity::check(&part).unwrap();
    let want = 60.0 * 40.0 * 10.0 + PI * 100.0 * 12.0;
    let got = volume(&k, &mut part);
    let off = rim_off_circle(&k, &part);
    eprintln!("exact mates {exact_mates}: volume {got}, closed form {want}, rim off {off:e}");
    ((got - want) / want, off)
}

#[test]
fn issue_1061_moved_boss_top_is_exact_with_exact_mate_edges() {
    let (rel, off) = moved_top(true);
    // measured 1.2e-14 and 3.6e-15
    assert!(rel.abs() < 1e-12, "{rel}");
    assert!(off < 1e-12, "{off}");
}

/// Without the opt-in the rim is marched off the circle and the part comes out short
/// (measured -9.7e-9 and 7.5e-7), as upstream marches it.
#[test]
fn issue_1061_without_the_opt_in_the_rim_is_marched() {
    let (rel, off) = moved_top(false);
    assert!(rel < -1e-9, "{rel}");
    assert!(off > 1e-8, "{off}");
}
