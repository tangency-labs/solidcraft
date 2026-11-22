//! Regression test for #431 (fork `ef9041d`): the level-2 check of a plate with many
//! teeth along one side, whose caps are bounded by `4 n + 4` edges. The check's
//! per-face pair searches and vertex classifications visit only the candidates a box
//! search leaves possible; the reports stay upstream's (the pair searches are compared
//! with all-pairs searches in `topology::face::assert_tests`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use solidcore::{Kernel, Vec3};

/// The plate of the fork's `tests/check.cpp`: one profile of lines with `teeth` teeth,
/// swept 2 along z and capped.
fn toothed_plate(k: &mut Kernel, teeth: u32) -> Brep {
    let length = 2.0 * f64::from(teeth) + 1.0;
    let mut points = vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(length, 0.0, 0.0),
        Vec3::new(length, 5.0, 0.0),
    ];
    for i in (0..teeth).rev() {
        let x = 2.0 * f64::from(i) + 1.0;
        points.push(Vec3::new(x + 1.0, 5.0, 0.0));
        points.push(Vec3::new(x + 1.0, 6.0, 0.0));
        points.push(Vec3::new(x, 6.0, 0.0));
        points.push(Vec3::new(x, 5.0, 0.0));
    }
    points.push(Vec3::new(0.0, 5.0, 0.0));
    let curves: Vec<Curve> = (0..points.len())
        .map(|i| {
            let next = points[(i + 1) % points.len()];
            Curve::from(create_line_segment(points[i], next).unwrap())
        })
        .collect();
    let curves: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &curves, Vec3::new(0.0, 0.0, 1.0), 2.0, true).unwrap()
}

/// A 60-tooth plate (246 faces: a side per profile line and two caps) checks clean at
/// level 2.
#[test]
fn toothed_plate_checks_clean_at_level_2() {
    let mut k = Kernel::default();
    let mut b = toothed_plate(&mut k, 60);
    // the face list, not the face arena: each cap is cut from a face over its whole
    // plane (`CreateFacesWithCurves`), which is then deleted, unlisted but kept in the
    // arena (`destroy_face`), so the arena holds two more records
    assert_eq!(b.face_list().len(), 4 * 60 + 6);
    let c = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert_eq!(c.failure_count(), 0, "{:?}", c.lines());
    assert!(c.is_valid());
}
