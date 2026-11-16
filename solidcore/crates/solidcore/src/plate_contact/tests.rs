//! Tests of the plate-contact maths on fixture F of the Tab and Slot design: `Bottom`
//! (0,0,0)–(160,100,6) and `Wall` (20,30,6)–(140,36,66), `t = 6`. Every number is worked by hand
//! beside its assertion.

use super::*;

fn plane(px: f64, py: f64, pz: f64, nx: f64, ny: f64, nz: f64) -> Plane {
    Plane::through(Vec3::new(px, py, pz), Vec3::new(nx, ny, nz)).expect("plane")
}

/// `P`: Bottom's top face, z = 6, outward +z.
fn p() -> Plane {
    plane(0.0, 0.0, 6.0, 0.0, 0.0, 1.0)
}

/// Wall's six faces, outward.
fn wall() -> [Plane; 6] {
    [
        plane(20.0, 0.0, 0.0, -1.0, 0.0, 0.0),
        plane(140.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        plane(0.0, 30.0, 0.0, 0.0, -1.0, 0.0),
        plane(0.0, 36.0, 0.0, 0.0, 1.0, 0.0),
        plane(0.0, 0.0, 6.0, 0.0, 0.0, -1.0),
        plane(0.0, 0.0, 66.0, 0.0, 0.0, 1.0),
    ]
}

/// Σ, the strip where Wall stands on Bottom: x ∈ [20, 140], y ∈ [30, 36] (its extent along an
/// axis direction).
fn strip(m: Vec3) -> Result<(f64, f64)> {
    let corners = [(20.0, 30.0), (140.0, 30.0), (20.0, 36.0), (140.0, 36.0)];
    let levels = corners.map(|(x, y)| m.dot(Vec3::new(x, y, 6.0)));
    let lo = levels.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = levels.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Ok((lo, hi))
}

const TOL: f64 = 1.0e-3;

#[test]
fn plane_through_holds_its_level() {
    // Wall's face y = 36, outward +y: offset 36.
    assert_eq!(plane(0.0, 36.0, 0.0, 0.0, 1.0, 0.0).offset, 36.0);
    // Wall's face x = 20, outward −x: (−1, 0, 0)·(20, ·, ·) = −20.
    assert_eq!(plane(20.0, 0.0, 0.0, -1.0, 0.0, 0.0).offset, -20.0);
    // A normal given at length 2 is unitized: (0, 0, 2) through z = 6 gives offset 6.
    assert_eq!(plane(0.0, 0.0, 6.0, 0.0, 0.0, 2.0).offset, 6.0);
}

#[test]
fn bearings() {
    assert_eq!(bearing(Vec3::Z, Vec3::Z).unwrap(), Bearing::Same);
    assert_eq!(bearing(Vec3::Z, -Vec3::Z).unwrap(), Bearing::Opposite);
    assert_eq!(bearing(Vec3::Z, Vec3::X).unwrap(), Bearing::Square);
    // A wall tilted 5° from upright: its side face's normal is (cos 5°, 0, sin 5°), 85° from +z.
    let tilt = 5.0_f64.to_radians();
    let side = Vec3::new(tilt.cos(), 0.0, tilt.sin());
    match bearing(Vec3::Z, side).unwrap() {
        Bearing::Oblique(deg) => assert!((deg - 85.0).abs() < 1e-9, "{deg}"),
        b => panic!("{b:?}"),
    }
}

#[test]
fn bearing_slack_boundary() {
    // Just inside and just outside SQUARE_SLACK of square: 90° ± (slack ∓ 10⁻⁷ rad).
    let inside = SQUARE_SLACK - 1.0e-7;
    let outside = SQUARE_SLACK + 1.0e-7;
    let v = |a: f64| Vec3::new(a.cos(), 0.0, a.sin());
    assert_eq!(bearing(Vec3::Z, v(inside)).unwrap(), Bearing::Square);
    assert!(matches!(
        bearing(Vec3::Z, v(outside)).unwrap(),
        Bearing::Oblique(_)
    ));
    // And of parallel.
    let w = |a: f64| Vec3::new(a.sin(), 0.0, a.cos());
    assert_eq!(bearing(Vec3::Z, w(inside)).unwrap(), Bearing::Same);
    assert!(matches!(
        bearing(Vec3::Z, w(outside)).unwrap(),
        Bearing::Oblique(_)
    ));
}

#[test]
fn bearing_refuses_non_unit_and_nan() {
    assert!(bearing(Vec3::Z, Vec3::new(0.0, 0.0, 2.0)).is_err());
    assert!(bearing(Vec3::Z, Vec3::new(f64::NAN, 0.0, 1.0)).is_err());
    assert!(Plane::through(Vec3::ZERO, Vec3::ZERO).is_err());
    assert!(Plane::through(Vec3::new(f64::INFINITY, 0.0, 0.0), Vec3::Z).is_err());
}

#[test]
fn touch_within_tol() {
    // Design 16.1 "Touch within tol", tol 10⁻³: T's end face (outward −z) at z = 6 ± 5·10⁻⁴
    // touches with δ = ±5·10⁻⁴; at 6 + 2·10⁻³ it does not.
    let end = |z: f64| plane(0.0, 0.0, z, 0.0, 0.0, -1.0);
    assert_eq!(touch_offset(end(6.0), p(), TOL).unwrap(), Some(0.0));
    let up = touch_offset(end(6.0005), p(), TOL).unwrap().unwrap();
    assert!((up - 5.0e-4).abs() < 1e-12, "{up}");
    let down = touch_offset(end(5.9995), p(), TOL).unwrap().unwrap();
    assert!((down + 5.0e-4).abs() < 1e-12, "{down}");
    assert_eq!(touch_offset(end(6.002), p(), TOL).unwrap(), None);
    assert_eq!(touch_offset(end(5.998), p(), TOL).unwrap(), None);
}

#[test]
fn touch_needs_the_face_to_face_down() {
    // Wall's top (outward +z) at z = 6 would face the same way as P: no touch.
    assert_eq!(
        touch_offset(plane(0.0, 0.0, 6.0, 0.0, 0.0, 1.0), p(), TOL).unwrap(),
        None
    );
    // A side face square to P: no touch.
    assert_eq!(touch_offset(wall()[2], p(), TOL).unwrap(), None);
    assert!(touch_offset(wall()[4], p(), -1.0).is_err());
    assert!(touch_offset(wall()[4], p(), f64::NAN).is_err());
}

#[test]
fn wall_plate_faces_are_the_narrow_pair() {
    // F: the x pair is 140 − 20 = 120 apart (it bounds the strip along its length); the y pair is
    // 36 − 30 = 6 apart: the narrower pair wins, t_T = 6, m = +y.
    let got = plate_faces(&wall(), Vec3::Z, TOL, strip).unwrap().unwrap();
    assert_eq!(got.thickness, 6.0);
    assert_eq!(got.b.normal, Vec3::Y);
    assert_eq!(got.a.normal, -Vec3::Y);
    assert_eq!(got.a.offset, -30.0);
    assert_eq!(got.b.offset, 36.0);
}

#[test]
fn wall_plate_faces_any_order() {
    // The same answer whichever of the pair comes first.
    let mut faces = wall();
    faces.swap(2, 3);
    let got = plate_faces(&faces, Vec3::Z, TOL, strip).unwrap().unwrap();
    assert_eq!(got.thickness, 6.0);
    assert_eq!(got.b.normal, Vec3::Y);
}

#[test]
fn square_post_is_not_a_plate() {
    // A 6 × 6 post at x ∈ [20, 26], y ∈ [30, 36]: both pairs are 6 thick and bound its strip.
    let post = [
        plane(20.0, 0.0, 0.0, -1.0, 0.0, 0.0),
        plane(26.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        plane(0.0, 30.0, 0.0, 0.0, -1.0, 0.0),
        plane(0.0, 36.0, 0.0, 0.0, 1.0, 0.0),
    ];
    let square = |m: Vec3| -> Result<(f64, f64)> {
        let c = [(20.0, 30.0), (26.0, 30.0), (20.0, 36.0), (26.0, 36.0)];
        let l = c.map(|(x, y)| m.dot(Vec3::new(x, y, 6.0)));
        Ok((
            l.iter().copied().fold(f64::INFINITY, f64::min),
            l.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ))
    };
    assert_eq!(
        plate_faces(&post, Vec3::Z, TOL, square).unwrap(),
        Err(NoPlateFaces::NotAPlate)
    );
}

#[test]
fn no_square_faces_no_pair() {
    // Only the end and top faces (parallel to n): nothing square to n.
    let faces = [wall()[4], wall()[5]];
    assert_eq!(
        plate_faces(&faces, Vec3::Z, TOL, strip).unwrap(),
        Err(NoPlateFaces::NoPair)
    );
}

#[test]
fn plate_faces_must_bound_the_strip() {
    // A strip narrower than the pair (y ∈ [31, 36]: T notched under one face): the y pair's
    // level −(−30) = 30 is 1 from the strip's 31, over tol, so the y pair does not qualify.
    // The strip still runs x ∈ [20, 140], the x pair's levels, so of all six faces the x pair
    // (120 thick) is the one left (design v2 5.4: a pair qualifies on its own direction only).
    let narrow = |m: Vec3| -> Result<(f64, f64)> {
        let c = [(20.0, 31.0), (140.0, 31.0), (20.0, 36.0), (140.0, 36.0)];
        let l = c.map(|(x, y)| m.dot(Vec3::new(x, y, 6.0)));
        Ok((
            l.iter().copied().fold(f64::INFINITY, f64::min),
            l.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ))
    };
    let got = plate_faces(&wall(), Vec3::Z, TOL, narrow).unwrap().unwrap();
    assert_eq!(got.thickness, 120.0);
    assert_eq!(got.b.normal, Vec3::X);
    // With the y pair alone: no pair.
    let y_pair = [wall()[2], wall()[3]];
    assert_eq!(
        plate_faces(&y_pair, Vec3::Z, TOL, narrow).unwrap(),
        Err(NoPlateFaces::NoPair)
    );
}

#[test]
fn plate_faces_refuses_bad_input() {
    assert!(plate_faces(&wall(), Vec3::new(0.0, 0.0, 2.0), TOL, strip).is_err());
    assert!(plate_faces(&wall(), Vec3::Z, f64::INFINITY, strip).is_err());
    let nan = |_: Vec3| -> Result<(f64, f64)> { Ok((f64::NAN, 1.0)) };
    assert!(plate_faces(&wall(), Vec3::Z, TOL, nan).is_err());
}

#[test]
fn contact_is_touch_then_overlap_then_the_distance() {
    let never = || -> Result<f64> { panic!("the distance is asked for only without contact") };
    // Wall on Bottom, its foot 2e-4 under P: a touch at that level.
    assert_eq!(
        contact(true, Some(-2.0e-4), true, TOL, never).unwrap(),
        Contact::Touch { delta: -2.0e-4 }
    );
    // A touch level whose face covers none of S's face does not count; the crossing does.
    assert_eq!(
        contact(false, Some(0.0), true, TOL, never).unwrap(),
        Contact::Overlap
    );
    // Neither: within tol is an end that is not flat, beyond it no contact with the distance.
    assert_eq!(
        contact(false, None, false, TOL, || Ok(5.0e-4)).unwrap(),
        Contact::EndNotFlat
    );
    assert_eq!(
        contact(false, None, false, TOL, || Ok(4.0)).unwrap(),
        Contact::NoContact { distance: 4.0 }
    );
    // Refusals: a touch level outside tol, a NaN distance, a negative tol.
    assert!(contact(true, Some(2.0e-3), false, TOL, never).is_err());
    assert!(contact(false, None, false, TOL, || Ok(f64::NAN)).is_err());
    assert!(contact(true, Some(0.0), false, -1.0, never).is_err());
}

#[test]
fn a_hole_across_the_strip_splits_it_and_a_sliver_does_not() {
    // Wall's strip runs u in [20, 140]; a 20 mm hole in Bottom across it at u in [70, 90] (wider
    // than the strip, so 6 across) leaves [20, 70] and [90, 140].
    let hole = Defect {
        along: (70.0, 90.0),
        across: 6.0,
    };
    assert_eq!(
        stretches((20.0, 140.0), &[hole], TOL).unwrap(),
        vec![(20.0, 70.0), (90.0, 140.0)]
    );
    // A STEP end face 5e-4 off A's plane leaves a sliver 5e-4 across the whole strip: ignored.
    let sliver = Defect {
        along: (20.0, 140.0),
        across: 5.0e-4,
    };
    assert_eq!(
        stretches((20.0, 140.0), &[sliver, hole], TOL).unwrap(),
        vec![(20.0, 70.0), (90.0, 140.0)]
    );
    // Overlapping defects, given out of order, and one reaching past the end; a piece left no
    // longer than tol (the 5e-4 at the start) is dropped.
    let defects = [
        Defect {
            along: (100.0, 150.0),
            across: 6.0,
        },
        Defect {
            along: (20.0005, 40.0),
            across: 6.0,
        },
        Defect {
            along: (30.0, 50.0),
            across: 1.0,
        },
    ];
    assert_eq!(
        stretches((20.0, 140.0), &defects, TOL).unwrap(),
        vec![(50.0, 100.0)]
    );
    // No defects: one stretch; refusals for disordered or non-finite numbers.
    assert_eq!(
        stretches((20.0, 140.0), &[], TOL).unwrap(),
        vec![(20.0, 140.0)]
    );
    assert!(stretches((140.0, 20.0), &[], TOL).is_err());
    assert!(stretches(
        (20.0, 140.0),
        &[Defect {
            along: (1.0, f64::NAN),
            across: 1.0
        }],
        TOL
    )
    .is_err());
}

#[test]
fn the_strip_reaches_as_far_as_its_region_on_the_frame() {
    // Σ of F as a region on P's plane, frame origin (0, 0, 6), x and y the world axes:
    // the rectangle (20, 30)–(140, 36). Along +y: 30..36; along −x: −140..−20; plate_faces on
    // it gives F's y pair, 6 thick.
    use crate::eval::Frame;
    use crate::math::Point2;
    use crate::prism::profile::{Loop2, Region2, Seg2};
    let c = [
        Point2::new(20.0, 30.0),
        Point2::new(140.0, 30.0),
        Point2::new(140.0, 36.0),
        Point2::new(20.0, 36.0),
    ];
    let sigma = Region2 {
        outer: Loop2 {
            segs: (0..4)
                .map(|i| Seg2::Line {
                    a: c[i],
                    b: c[(i + 1) % 4],
                })
                .collect(),
        },
        holes: vec![],
    };
    let frame = Frame {
        origin: [0.0, 0.0, 6.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    };
    let eps = 1e-9;
    assert_eq!(
        strip_extent(&sigma, &frame, Vec3::Y, eps).unwrap(),
        (30.0, 36.0)
    );
    assert_eq!(
        strip_extent(&sigma, &frame, -Vec3::X, eps).unwrap(),
        (-140.0, -20.0)
    );
    assert!(strip_extent(&sigma, &frame, Vec3::Z, eps).is_err());
    let got = plate_faces(&wall(), Vec3::Z, TOL, |m| {
        strip_extent(&sigma, &frame, m, eps)
    })
    .unwrap()
    .unwrap();
    assert_eq!(got.thickness, 6.0);
    assert_eq!(got.b.normal, Vec3::Y);
}
