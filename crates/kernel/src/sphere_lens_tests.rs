use super::*;
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

/// A flat-backed ring with a shallow circular crown, revolved about Y (#3192).
fn crowned_ring() -> Body {
    let start = Vec2::new(50., 0.);
    let end = Vec2::new(15., 0.);
    let center = Vec2::new(32.5, -18.375);
    let radius = 25.375;
    let first = (start - center).angle();
    let last = (end - center).angle();
    let region = Region2 {
        outer: Loop2 { segs: vec![Seg2::Line { a: end, b: start }, Seg2::Arc { center, radius, start: first, sweep: last - first }] },
        holes: vec![],
    };
    crate::revolve_whole(&Plane::XY, &[region], Vec2::ZERO, Vec2::Y, 2. * PI).unwrap().pop().unwrap()
}

fn ring_lens(radius: f64) -> Region2 {
    crate::planar_regions_boolean(
        &[Region2 { outer: Loop2::circle(Vec2::ZERO, radius), holes: vec![] }],
        &[Region2 { outer: Loop2::circle(Vec2::new(-20., 35.), 20.), holes: vec![] }],
        BoolOp::Intersect,
        1e-7,
    )
    .unwrap()
    .pop()
    .unwrap()
}

#[test]
fn a_revolved_ring_outline_does_not_block_a_symmetric_lens_cut() {
    let target = crowned_ring();
    let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    let cutter = crate::extrude(&plane, &[ring_lens(50.)], -9., 9.).unwrap().pop().unwrap();
    assert!(enclosed_in_cylinder(&target, plane, Vec2::ZERO, 50., 1e-7));
    let relaxed = relaxed_tool(&target, &cutter).expect("the outer cylinder only touches the ring's rim");
    let bounds = crate::measure(&relaxed).unwrap().bbox;
    assert!((bounds.min.y + 9.).abs() < 1e-6 && (bounds.max.y - 9.).abs() < 1e-6);
    let (result, diagnostics) = crate::capture_boolean_diagnostics(|| crate::boolean(&target, &cutter, BoolOp::Cut));
    let result = result.unwrap().unwrap();
    assert!(diagnostics.events.iter().all(|event| !event.strategy.alters_result()), "{diagnostics:?}");
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    assert_eq!(result.lumps().unwrap().len(), 1);
    let mesh = result.tessellate_with(1e-4, true).unwrap();
    // Integrate rho * crown_height(rho) * the remaining angular span.
    let intervals = 20_000;
    let distance = 20_f64.hypot(35.);
    let integrand = |rho: f64| {
        let crown = (25.375_f64.powi(2) - (rho - 32.5).powi(2)).max(0.).sqrt() - 18.375;
        let cut = 2. * ((rho * rho + distance * distance - 400.) / (2. * rho * distance)).clamp(-1., 1.).acos();
        rho * crown * (2. * PI - cut)
    };
    let step = 35. / intervals as f64;
    let want: f64 = (0..=intervals)
        .map(|i| {
            let weight = if i == 0 || i == intervals {
                1.
            } else if i % 2 == 0 {
                2.
            } else {
                4.
            };
            weight * integrand(15. + i as f64 * step)
        })
        .sum::<f64>()
        * step
        / 3.;
    let got = mesh.measure().volume;
    assert!(!result.contains(Vec3::new(-20., 2., 35.)));
    assert!(result.contains(Vec3::new(30., 2., 0.)));
    let history = result.history().expect("the replacement cylinder keeps tool face history");
    history.check(&result, &[&target, &cutter]).unwrap();
    for face in 0..cutter.face_count() {
        if matches!(face_surface(&cutter, face), Some(FaceSurface::Cylinder { radius, .. }) if (radius - 20.).abs() < 1e-7) {
            assert!(
                history.faces.iter().any(|entry| entry.origins.contains(&FaceOrigin::Face { operand: 1, face })),
                "the cutting circle's seam pieces keep their original faces: {face}"
            );
        }
    }
    // The Boolean traces numerical intersection curves. One mm³ is below
    // 0.004% of this part; the independent material and history checks above
    // also reject a shifted cut or an uncut ring.
    assert!((got - want).abs() < 1.0, "{got} vs {want}");
}

#[test]
fn a_ring_lens_keeps_material_outside_a_smaller_outline_and_beyond_its_depth() {
    let target = crowned_ring();
    let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    let smaller = crate::extrude(&plane, &[ring_lens(49.)], -9., 9.).unwrap().pop().unwrap();
    assert!(relaxed_tool(&target, &smaller).is_none(), "the 49 mm wall removes real ring material");
    let shallow = crate::extrude(&plane, &[ring_lens(50.)], -3., 1.).unwrap().pop().unwrap();
    let relaxed = relaxed_tool(&target, &shallow).unwrap();
    let bounds = crate::measure(&relaxed).unwrap().bbox;
    assert!((bounds.min.y + 1.).abs() < 1e-6 && (bounds.max.y - 3.).abs() < 1e-6);
    let cut = crate::boolean(&target, &shallow, BoolOp::Cut).unwrap().unwrap();
    assert!(!cut.contains(Vec3::new(-20., 2., 35.)), "the shallow pocket removes its requested depth");
    assert!(cut.contains(Vec3::new(-20., 4., 35.)), "material beyond the pocket floor stays");
    assert!(!enclosed_in_cylinder(&target, Plane::XY, Vec2::ZERO, 50., 1e-7), "a transverse axis is not the revolution axis");
}

#[test]
fn union_outlines_and_meridians_that_bulge_between_seams_are_not_relaxed() {
    let target = crowned_ring();
    let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    let region = crate::planar_regions_boolean(
        &[Region2 { outer: Loop2::circle(Vec2::ZERO, 50.), holes: vec![] }],
        &[Region2 { outer: Loop2::circle(Vec2::new(-20., 35.), 20.), holes: vec![] }],
        BoolOp::Union,
        1e-7,
    )
    .unwrap()
    .pop()
    .unwrap();
    let cutter = crate::extrude(&plane, &[region], -9., 9.).unwrap().pop().unwrap();
    assert!(relaxed_tool(&target, &cutter).is_none(), "a union cuts more than either supporting circle");

    // Both seam radii are 40 mm, but the quadratic meridian reaches 55 mm
    // between them. Looking only at face vertices would falsely certify it.
    let profile = Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Conic { a: Vec2::new(40., 0.), apex: Vec2::new(70., 5.), b: Vec2::new(40., 10.), w: 1. },
                Seg2::Line { a: Vec2::new(40., 10.), b: Vec2::new(40., 0.) },
            ],
        },
        holes: vec![],
    };
    let bulging = crate::revolve_whole(&Plane::XY, &[profile], Vec2::ZERO, Vec2::Y, 2. * PI).unwrap().pop().unwrap();
    assert!(!enclosed_in_cylinder(&bulging, plane, Vec2::ZERO, 50., 1e-7));
}
