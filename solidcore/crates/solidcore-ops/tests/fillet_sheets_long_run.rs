//! The fillet along a long shoulder: two sheets lofted between curves fitted through five
//! points each (a car body's side, from the shoulder down to the sill, and its deck, from the
//! roof line to the shoulder), sharing the shoulder curve, filleted at the tolerance a surface
//! modeller sews at (0.01). The sheets' ends cross the shoulder obliquely, so each round runs
//! on past them on the faces' extensions before it is cut back.
//!
//! A round or curvature-continuous fillet is made as one face, both sheets cut back to it.
//! Before, a small round's fillet face was taken for a sheet's (one point inside it lay on the
//! sheet) and the fillet was refused as making nothing, and every curvature-continuous round
//! was refused with the kernel's bare status 1001: its stations past the ends were solved on
//! the faces' end spans carried on, not on the extensions the trace ran on.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::blend_surface::{fillet_sheets, Fillet, FilletSheets, Section};
use solidcore_ops::continuity::ContinuityTolerances;
use solidcore_ops::curve_fit::ep_curve;
use solidcore_ops::sweep::{loft, Curve};
use solidcore_ops::Body;

const SHOULDER: [[f64; 3]; 5] = [
    [77.3, -900.0, 407.9],
    [1121.5, -900.0, 858.7],
    [1936.4, -900.0, 713.5],
    [2955.1, -900.0, 828.1],
    [3770.0, -900.0, 975.8],
];
const SILL: [[f64; 3]; 5] = [
    [300.0, -900.0, 330.0],
    [1885.5, -900.0, 484.3],
    [2827.8, -900.0, 624.3],
    [3337.2, -900.0, 688.0],
    [3768.0, -900.0, 547.9],
];
const ROOF: [[f64; 3]; 5] = [
    [1197.9, 0.0, 968.2],
    [2063.7, 0.0, 1133.7],
    [3082.4, 0.0, 1108.2],
    [3846.4, 0.0, 1006.4],
    [4419.4, 0.0, 896.8],
];

/// The cubic through `points` (the operations' fit), as a loft section.
fn fit(points: &[[f64; 3]]) -> Curve {
    let (c, _) = ep_curve(points, 3).unwrap();
    let solidcore_record::brep::Curve::Spline { knots, points } = c else {
        panic!("a fit through points is a spline");
    };
    Curve::Nurbs {
        degree: knots.len() - points.len() - 1,
        points: points.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect(),
        knots,
    }
}

/// The deck (roof to shoulder) and the side (shoulder to sill), each through its record as a
/// document holds it.
fn deck_and_side() -> (Body, Body) {
    let through_record = |s: Body| {
        Body::from_record(&s.to_record().unwrap().0, true)
            .unwrap()
            .0
    };
    let deck = loft(&[vec![fit(&ROOF)], vec![fit(&SHOULDER)]], 1, false).unwrap();
    let side = loft(&[vec![fit(&SHOULDER)], vec![fit(&SILL)]], 1, false).unwrap();
    (through_record(deck), through_record(side))
}

fn how(section: Section) -> FilletSheets {
    FilletSheets {
        section,
        trim: true,
        tolerance: 1e-2,
        stations: 17,
        seam: ContinuityTolerances {
            gap_mm: 1e-2,
            angle_rad: 1e-2,
            curvature_per_mm: 1e-2,
            curvature_change_per_mm2: 0.0,
            curvature_change_rate_per_mm3: 0.0,
        },
    }
}

/// One fillet face, both sheets cut back, and its two seams holding `order` at every station.
fn holds(f: &Fillet, order: usize) {
    assert_eq!(f.fillet.to_record().unwrap().0.faces.len(), 1);
    assert!(f.trimmed.is_some());
    assert_eq!(f.seams.len(), 2);
    for seam in &f.seams {
        assert_eq!(seam.order_at_stations, Some(order), "{seam:?}");
    }
}

#[test]
fn a_round_of_radius_5_along_the_shoulder_is_one_face_tangent_to_both_sheets() {
    let (deck, side) = deck_and_side();
    let f = fillet_sheets(&deck, &side, &how(Section::Radius(5.0))).unwrap();
    holds(&f, 1);
}

#[test]
#[ignore = "the curvature-continuous round's certificate along the 6.4 m run takes about 5 \
            minutes optimised; run with --ignored under the goldens profile"]
fn a_curvature_continuous_round_along_the_shoulder_is_curvature_continuous_with_both_sheets() {
    let (deck, side) = deck_and_side();
    let f = fillet_sheets(&deck, &side, &how(Section::Curvature(30.0))).unwrap();
    holds(&f, 2);
}
