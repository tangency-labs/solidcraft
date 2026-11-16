//! A sheet's principal curvatures at points, and how far a curve strays from a curve or a sheet,
//! through ops on records: a sphere reads 1/r twice, a cylinder 1/r and 0, a saddle +1 and -1;
//! two lines 5 apart deviate by 5, a curve on a flat sheet by 0, a 0.0056 offset reads back.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::continuity::principal_curvatures;
use solidcore_ops::curve::{curve_deviation, curve_surface_deviation};
use solidcore_ops::surface::{reverse_sheet, ruled};
use solidcore_ops::{make_cylinder, make_sphere, Error};
use solidcore_record::brep::Curve;

fn line(a: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![a, b],
    }
}

fn cubic(points: [[f64; 3]; 4]) -> Curve {
    Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: points.to_vec(),
    }
}

#[test]
fn a_sphere_reads_its_inverse_radius_twice() {
    let (sphere, _) = make_sphere([0.0; 3], 4.0).unwrap().to_record().unwrap();
    let got = principal_curvatures(&sphere, &[[3.0, 4.0, 5.0]]).unwrap();
    let p = got[0].unwrap();
    assert!(
        (p.k1 - 0.25).abs() < 1e-9 && (p.k2 - 0.25).abs() < 1e-9,
        "{p:?}"
    );
    assert!((p.gaussian - 1.0 / 16.0).abs() < 1e-9, "{p:?}");
    assert!((p.mean - 0.25).abs() < 1e-9, "{p:?}");
}

#[test]
fn a_sphere_has_no_curvature_value_at_its_poles() {
    let (sphere, _) = make_sphere([0.0; 3], 4.0).unwrap().to_record().unwrap();
    let got = principal_curvatures(
        &sphere,
        &[
            [0.0, 0.0, 4.0],
            [0.0, 0.0, -4.0],
            [0.0, 0.0, 6.0],
            [0.0, 0.0, -6.0],
        ],
    )
    .unwrap();
    assert_eq!(got, vec![None; 4]);
}

#[test]
fn a_cylinder_reads_its_inverse_radius_round_and_nothing_along() {
    let (can, _) = make_cylinder([0.0; 3], 3.0, 10.0)
        .unwrap()
        .to_record()
        .unwrap();
    let got = principal_curvatures(&can, &[[5.0, 0.0, 5.0]]).unwrap();
    let p = got[0].unwrap();
    assert!((p.k1 - 1.0 / 3.0).abs() < 1e-9, "{p:?}");
    assert!(p.k2.abs() < 1e-9, "{p:?}");
    assert!(p.gaussian.abs() < 1e-9, "{p:?}");
    assert!((p.mean - 1.0 / 6.0).abs() < 1e-9, "{p:?}");
}

/// The saddle z = x y (the ruled sheet between two crossed lines) bends up one way and down the
/// other at its middle, by 1 each.
#[test]
fn a_saddle_reads_one_curvature_up_and_one_down() {
    let saddle = ruled(
        &line([-1.0, -1.0, 1.0], [1.0, -1.0, -1.0]),
        &line([-1.0, 1.0, -1.0], [1.0, 1.0, 1.0]),
        false,
    )
    .unwrap();
    let got = principal_curvatures(&saddle, &[[0.0, 0.0, 0.5]]).unwrap();
    let p = got[0].unwrap();
    assert!(p.k1 > 0.0 && p.k2 < 0.0, "{p:?}");
    assert!(
        (p.k1 - 1.0).abs() < 1e-9 && (p.k2 + 1.0).abs() < 1e-9,
        "{p:?}"
    );
    assert!((p.gaussian + 1.0).abs() < 1e-9, "{p:?}");
    assert!(p.mean.abs() < 1e-9, "{p:?}");

    // At x = y = 1/4, K = -64/81 and |H| = sqrt(2)/27; reversal changes only H's sign.
    let points = [[0.25, 0.25, 0.0625]];
    let before = principal_curvatures(&saddle, &points).unwrap()[0].unwrap();
    assert!((before.gaussian + 64.0 / 81.0).abs() < 1e-9, "{before:?}");
    assert!(
        (before.mean.abs() - std::f64::consts::SQRT_2 / 27.0).abs() < 1e-9,
        "{before:?}"
    );
    let (reversed, _) = reverse_sheet(&saddle).unwrap();
    let after = principal_curvatures(&reversed, &points).unwrap()[0].unwrap();
    assert!(
        (after.gaussian - before.gaussian).abs() < 1e-9,
        "{before:?} {after:?}"
    );
    assert!(
        (after.mean + before.mean).abs() < 1e-9,
        "{before:?} {after:?}"
    );
    assert!((after.k1 + before.k2).abs() < 1e-9, "{before:?} {after:?}");
    assert!((after.k2 + before.k1).abs() < 1e-9, "{before:?} {after:?}");
    assert_eq!(
        principal_curvatures(&saddle, &[[f64::NAN, 0.0, 0.0]]).err(),
        Some(Error::Argument)
    );
}

#[test]
fn a_plane_has_zero_curvature_instead_of_no_value() {
    let plane = ruled(
        &line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
        &line([0.0, 10.0, 0.0], [10.0, 10.0, 0.0]),
        false,
    )
    .unwrap();
    let p = principal_curvatures(&plane, &[[5.0, 5.0, 1.0]]).unwrap()[0].unwrap();
    assert!(p.k1.abs() < 1e-9 && p.k2.abs() < 1e-9, "{p:?}");
    assert!(p.gaussian.abs() < 1e-9 && p.mean.abs() < 1e-9, "{p:?}");
}

#[test]
fn two_lines_five_apart_deviate_by_five() {
    let d = curve_deviation(
        &line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
        &line([0.0, 5.0, 0.0], [10.0, 5.0, 0.0]),
        11,
    )
    .unwrap();
    assert!((d.max - 5.0).abs() < 1e-9, "{d:?}");
    assert_eq!(d.comb.len(), 11);
    let bound = d.bound.unwrap();
    assert!((bound - 5.0).abs() < 1e-9, "{bound}");
}

#[test]
fn a_curve_on_a_flat_sheet_deviates_by_nothing() {
    let plate = ruled(
        &line([0.0, 0.0, 0.0], [20.0, 0.0, 0.0]),
        &line([0.0, 20.0, 0.0], [20.0, 20.0, 0.0]),
        false,
    )
    .unwrap();
    let c = cubic([
        [1.0, 1.0, 0.0],
        [5.0, 9.0, 0.0],
        [12.0, 3.0, 0.0],
        [18.0, 15.0, 0.0],
    ]);
    let d = curve_surface_deviation(&c, &plate, 21).unwrap();
    assert!(d.max < 1e-9, "{d:?}");
    assert_eq!(d.bound, None);
}

/// A cubic and the same cubic 0.0056 higher: the gap reads back.
#[test]
fn a_gap_of_0_0056_reads_back() {
    let pts = [
        [0.0, 0.0, 0.0],
        [3.0, 4.0, 0.0],
        [7.0, -2.0, 0.0],
        [10.0, 1.0, 0.0],
    ];
    let up = pts.map(|p| [p[0], p[1], p[2] + 0.0056]);
    let d = curve_deviation(&cubic(pts), &cubic(up), 33).unwrap();
    assert!((d.max - 0.0056).abs() < 1e-9, "{d:?}");
    assert_eq!(
        curve_deviation(&cubic(pts), &cubic(up), 1).err(),
        Some(Error::Argument)
    );
}
