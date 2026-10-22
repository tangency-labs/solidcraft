//! A lens extruded from a sphere's projected outline cut from the sphere (#2750): one of its
//! cylinders encloses the target and only touches the sphere.
use crate::history::BodyHistory;
use crate::{Body, BoolOp};
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use std::f64::consts::{FRAC_PI_2, PI};

fn dome() -> Body {
    let profile = Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Arc { center: Vec2::ZERO, radius: 25., start: 0., sweep: PI },
                Seg2::Line { a: Vec2::new(-25., 0.), b: Vec2::new(25., 0.) },
            ],
        },
        holes: vec![],
    };
    let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    crate::revolve_whole(&plane, &[profile], Vec2::ZERO, Vec2::X, FRAC_PI_2).unwrap().pop().unwrap()
}

fn lens(enclosing_radius: f64) -> Region2 {
    crate::planar_regions_boolean(
        &[Region2 { outer: Loop2::circle(Vec2::ZERO, enclosing_radius), holes: vec![] }],
        &[Region2 { outer: Loop2::circle(Vec2::new(0., 20.), 15.), holes: vec![] }],
        BoolOp::Intersect,
        1e-7,
    )
    .unwrap()
    .pop()
    .unwrap()
}

fn tool(region: Region2, depth: f64) -> Body {
    let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    crate::extrude(&plane, &[region], 0., depth).unwrap().pop().unwrap()
}

#[test]
fn projected_spherical_outline_does_not_block_a_lens_cut() {
    let target = dome();
    let cutter = tool(lens(25.), 30.);
    let result = crate::boolean(&target, &cutter, BoolOp::Cut).unwrap().unwrap();
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    assert_eq!(result.lumps().unwrap().len(), 1);
    // Integrating the half-disc cross-section left by the offset bore gives
    // 8810.08973246 mm³ (R=25, bore radius=15, bore centre z=20).
    let measured = crate::measure(&result).unwrap();
    assert!((measured.volume - 8810.08973246).abs() < 2., "{measured:?}");
    assert!(measured.bbox.max.z < 20.01, "no thin lid may remain above the cut: {measured:?}");
    assert!(result.contains(Vec3::new(0., -10., 2.)));
    assert!(!result.contains(Vec3::new(0., -10., 15.)));
    assert_eq!(measured.merged.face_types.get("cylinder"), Some(&1));
    let history = result.history().expect("the cut has a face history");
    history.check(&result, &[&target, &cutter]).unwrap();
    assert!(history.faces.iter().all(|f| !f.origins.is_empty()));
}
