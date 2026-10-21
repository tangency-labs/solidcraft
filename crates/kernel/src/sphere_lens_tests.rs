use super::*;
use crate::history::BodyHistory;
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
    let (result, diagnostics) = crate::capture_boolean_diagnostics(|| crate::boolean(&target, &cutter, BoolOp::Cut));
    let result = result.unwrap().unwrap();
    assert!(diagnostics.events.iter().all(|event| !event.strategy.alters_result()), "{diagnostics:?}");
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
    let history = result.history().expect("the retry composes the tool's face history");
    history.check(&result, &[&target, &cutter]).unwrap();
    assert!(history.faces.iter().all(|f| !f.origins.is_empty()));
}

#[test]
fn only_an_enclosing_wall_can_be_removed_and_cap_depth_stays_exact() {
    let target = dome();
    assert!(relaxed_tool(&target, &tool(lens(24.), 30.)).is_none(), "a cylinder inside the sphere removes real material");
    let rectangle =
        Region2 { outer: Loop2::polygon(&[Vec2::new(-4., 5.), Vec2::new(4., 5.), Vec2::new(4., 15.), Vec2::new(-4., 15.)]), holes: vec![] };
    assert!(relaxed_tool(&target, &tool(rectangle, 30.)).is_none());
    let cutter = tool(lens(25.), 7.);
    let relaxed = relaxed_tool(&target, &cutter).unwrap();
    let bounds = crate::measure(&relaxed).unwrap().bbox;
    assert!((bounds.min.y + 7.).abs() < 1e-6 && bounds.max.y > 0., "only the end outside the quarter ball is extended: {bounds:?}");
    let cut = crate::boolean(&target, &cutter, BoolOp::Cut).unwrap().unwrap();
    assert!(cut.contains(Vec3::new(0., -8., 15.)), "material past the pocket floor stays");
    assert!(!cut.contains(Vec3::new(0., -6., 15.)), "the shallow pocket removes material up to its floor");
    let box_target = crate::box_solid(Vec3::new(-25., -25., 0.), Vec3::new(25., 0., 25.)).unwrap();
    assert!(relaxed_tool(&box_target, &cutter).is_none());
}
