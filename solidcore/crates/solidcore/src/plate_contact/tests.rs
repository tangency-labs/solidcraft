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
fn strip(m: Vec3) -> (f64, f64) {
    let corners = [(20.0, 30.0), (140.0, 30.0), (20.0, 36.0), (140.0, 36.0)];
    let levels = corners.map(|(x, y)| m.dot(Vec3::new(x, y, 6.0)));
    let lo = levels.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = levels.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (lo, hi)
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
    let got = plate_faces(&wall(), Vec3::Z, TOL, |m| Ok(strip(m)))
        .unwrap()
        .unwrap();
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
    let got = plate_faces(&faces, Vec3::Z, TOL, |m| Ok(strip(m)))
        .unwrap()
        .unwrap();
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
        plate_faces(&faces, Vec3::Z, TOL, |m| Ok(strip(m))).unwrap(),
        Err(NoPlateFaces::NoPair)
    );
}

#[test]
fn plate_faces_must_bound_the_strip() {
    // A strip narrower than the pair (y ∈ [31, 36]: T notched under one face): the y pair's
    // level −(−30) = 30 is 1 from the strip's 31, over tol, so the y pair does not qualify.
    let narrow = |m: Vec3| -> Result<(f64, f64)> {
        let c = [(20.0, 31.0), (140.0, 31.0), (20.0, 36.0), (140.0, 36.0)];
        let l = c.map(|(x, y)| m.dot(Vec3::new(x, y, 6.0)));
        Ok((
            l.iter().copied().fold(f64::INFINITY, f64::min),
            l.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ))
    };
    let y_pair = [wall()[2], wall()[3]];
    assert_eq!(
        plate_faces(&y_pair, Vec3::Z, TOL, narrow).unwrap(),
        Err(NoPlateFaces::NoPair)
    );
}

#[test]
fn a_notched_wall_is_not_read_along_its_length() {
    // Wall notched under its y = 30 face: the strip is x ∈ [20, 140], y ∈ [31, 36]. The y pair
    // fails to bound it (30 vs 31). The x pair bounds it along x (20 and 140), but it is
    // 140 − 20 = 120 thick and the strip is only 36 − 31 = 5 wide across it (along +z × +x = +y):
    // 120 > 5 + tol, so it is no plate thickness either. All six faces: no pair, not t_T = 120.
    let notched = |m: Vec3| -> Result<(f64, f64)> {
        let c = [(20.0, 31.0), (140.0, 31.0), (20.0, 36.0), (140.0, 36.0)];
        let l = c.map(|(x, y)| m.dot(Vec3::new(x, y, 6.0)));
        Ok((
            l.iter().copied().fold(f64::INFINITY, f64::min),
            l.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ))
    };
    assert_eq!(
        plate_faces(&wall(), Vec3::Z, TOL, notched).unwrap(),
        Err(NoPlateFaces::NoPair)
    );
    // The whole wall still reads as 6 thick: its strip is 120 long across the y pair (6 ≤ 120)
    // and the x pair, 120 thick, is wider than the 6 the strip spans across it.
    let got = plate_faces(&wall(), Vec3::Z, TOL, |m| Ok(strip(m)))
        .unwrap()
        .unwrap();
    assert_eq!(got.thickness, 6.0);
    assert_eq!(got.b.normal, Vec3::Y);
}

#[test]
fn plate_faces_refuses_bad_input() {
    assert!(plate_faces(&wall(), Vec3::new(0.0, 0.0, 2.0), TOL, |m| Ok(strip(m))).is_err());
    assert!(plate_faces(&wall(), Vec3::Z, f64::INFINITY, |m| Ok(strip(m))).is_err());
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

/// Wall of fixture F as a solid: (20, 30, 6)–(140, 36, 66), extruded along +z.
fn wall_solid(k: &mut crate::kernel::Kernel) -> crate::topology::Brep {
    use crate::math::Vec2;
    use crate::prism::profile::{Loop2, Region2, Seg2};
    let pts = [
        Vec2::new(20.0, 30.0),
        Vec2::new(140.0, 30.0),
        Vec2::new(140.0, 36.0),
        Vec2::new(20.0, 36.0),
    ];
    let outer = Loop2 {
        segs: pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .map(|(a, b)| Seg2::Line { a: *a, b: *b })
            .collect(),
    };
    let frame = frame_at([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let region = Region2 {
        outer,
        holes: vec![],
    };
    crate::prism::extrude::extrude_region(k, &frame, &region, 6.0, 66.0).expect("wall")
}

fn frame_at(origin: [f64; 3], x_axis: [f64; 3], y_axis: [f64; 3]) -> crate::eval::Frame {
    crate::eval::Frame {
        origin,
        x_axis,
        y_axis,
    }
}

fn areas(regions: &[crate::prism::profile::Region2]) -> Vec<f64> {
    regions
        .iter()
        .map(|r| r.outer.signed_area().expect("area").value)
        .collect()
}

/// `on` on P (z = 6, n = +z): Wall's bottom face, 120 × 6 = 720, counter-clockwise (positive).
/// Its top (z = 66) faces +z, not −n: nothing there. Seen from above with n = −z (x and y
/// swapped), the top face faces −n: 720 again. 0.5 mm off the bottom (tol 1e-3): nothing.
#[test]
fn the_footprint_on_p_is_the_faces_lying_in_it_facing_down() {
    let mut k = crate::kernel::Kernel::default();
    let wall = wall_solid(&mut k);
    let up = [1.0, 0.0, 0.0];
    let side = [0.0, 1.0, 0.0];
    let on = |origin, x, y| footprint_on(&k, &wall, &frame_at(origin, x, y), TOL).expect("on");
    let bottom = on([0.0, 0.0, 6.0], up, side);
    let a = areas(&bottom);
    assert!(a.len() == 1 && (a[0] - 720.0).abs() < 1e-9, "{a:?}");
    assert!(on([0.0, 0.0, 66.0], up, side).is_empty());
    let a = areas(&on([0.0, 0.0, 66.0], side, up));
    assert!(a.len() == 1 && (a[0] - 720.0).abs() < 1e-9, "{a:?}");
    assert!(on([0.0, 0.0, 6.5], up, side).is_empty());
    assert_eq!(on([0.0, 0.0, 6.0 + 0.5 * TOL], up, side).len(), 1);
    assert!(footprint_on(&k, &wall, &frame_at([0.0; 3], up, side), 0.0).is_err());
}

/// Wall on F (n = +z, strip along +x, t = 6). A 1 mm chamfer at Wall's foot: a 45° face
/// (normal (0, −1, −1)/√2) 1 mm tall, under 6: eased. The plate face itself (normal −y, square
/// to n): not eased. A 45° face 6 mm tall (no chamfer: the plate is tilted): not eased here
/// (`not_square` decides it). A 1 mm round: a cylinder along ±x: eased; along y: not.
#[test]
fn a_chamfer_or_a_round_at_the_foot_is_an_eased_edge() {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let chamfer = StripBound::Plane {
        normal: Vec3::new(0.0, -h, -h),
        height: 1.0,
    };
    let face = StripBound::Plane {
        normal: Vec3::new(0.0, -1.0, 0.0),
        height: 60.0,
    };
    let tilted = StripBound::Plane {
        normal: Vec3::new(0.0, -h, -h),
        height: 6.0,
    };
    let eased = |b: &[StripBound]| eased_edge(b, Vec3::Z, Vec3::X, 6.0, TOL).expect("eased");
    assert!(eased(&[face, chamfer]));
    assert!(!eased(&[face, face]));
    assert!(!eased(&[tilted]));
    assert!(eased(&[StripBound::Cylinder {
        axis: Vec3::new(-1.0, 0.0, 0.0)
    }]));
    assert!(!eased(&[StripBound::Cylinder { axis: Vec3::Y }]));
    assert!(!eased(&[StripBound::Other]));
    assert!(eased_edge(&[face], Vec3::Z, Vec3::X, 0.0, TOL).is_err());
}

/// `across` at z = 30 (n = +z): Wall's section, 120 × 6 = 720, one region, no hole,
/// counter-clockwise. Seen from below (x and y swapped, n = −z): 720 again, still
/// counter-clockwise in that frame. Above Wall (z = 100): nothing. tol 0: refused.
#[test]
fn the_footprint_across_p_is_the_section_of_t() {
    let mut k = crate::kernel::Kernel::default();
    let wall = wall_solid(&mut k);
    let up = [1.0, 0.0, 0.0];
    let side = [0.0, 1.0, 0.0];
    let across =
        |origin, x, y| footprint_across(&k, &wall, &frame_at(origin, x, y), TOL).expect("across");
    let mid = across([0.0, 0.0, 30.0], up, side);
    let a = areas(&mid);
    assert!(a.len() == 1 && (a[0] - 720.0).abs() < 1e-9, "{a:?}");
    assert!(mid.iter().all(|r| r.holes.is_empty()));
    let a = areas(&across([0.0, 0.0, 30.0], side, up));
    assert!(a.len() == 1 && (a[0] - 720.0).abs() < 1e-9, "{a:?}");
    assert!(across([0.0, 0.0, 100.0], up, side).is_empty());
    assert!(footprint_across(&k, &wall, &frame_at([0.0, 0.0, 30.0], up, side), 0.0).is_err());
}

/// A box `lo`–`hi` (axis-aligned), extruded along +z.
fn box_solid(k: &mut crate::kernel::Kernel, lo: [f64; 3], hi: [f64; 3]) -> crate::topology::Brep {
    use crate::math::Vec2;
    use crate::prism::profile::{Loop2, Region2, Seg2};
    let pts = [
        Vec2::new(lo[0], lo[1]),
        Vec2::new(hi[0], lo[1]),
        Vec2::new(hi[0], hi[1]),
        Vec2::new(lo[0], hi[1]),
    ];
    let outer = Loop2 {
        segs: pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .map(|(a, b)| Seg2::Line { a: *a, b: *b })
            .collect(),
    };
    let frame = frame_at([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let region = Region2 {
        outer,
        holes: vec![],
    };
    crate::prism::extrude::extrude_region(k, &frame, &region, lo[2], hi[2]).expect("box")
}

/// Design v2 16.1 [m22]: S stepped, 160 × 100, top P at z = 12: 12 thick under x ∈ [0, 80], 6
/// thick (bottom at z = 6) under x ∈ [80, 160]. Drawn in (z, x) and extruded along +y over
/// [0, 100]. T = Wall's footprint (x ∈ [20, 140], y ∈ [30, 36]) from z = 3 (9 below P) to 66:
/// its section at z = 6 is x ∈ [20, 140], sharing x ∈ [80, 140] with the thin part's bottom:
/// `passes_through`. Its section at z = 0 (the thick part's bottom) is empty. T from z = 9 (3
/// below P) reaches no far face: no. T over the thick part only (x ∈ [10, 70]), 9 deep: no.
/// T from z = 6, flush with the thin part's bottom: accepted (5.3). tol 0: refused.
#[test]
fn a_tab_running_through_the_thin_part_passes_through() {
    use crate::math::Vec2;
    use crate::prism::profile::{Loop2, Region2, Seg2};
    let mut k = crate::kernel::Kernel::default();
    // (u, v) = (z, x); n = z × x = +y.
    let pts = [
        Vec2::new(0.0, 0.0),
        Vec2::new(12.0, 0.0),
        Vec2::new(12.0, 160.0),
        Vec2::new(6.0, 160.0),
        Vec2::new(6.0, 80.0),
        Vec2::new(0.0, 80.0),
    ];
    let outer = Loop2 {
        segs: pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .map(|(a, b)| Seg2::Line { a: *a, b: *b })
            .collect(),
    };
    let region = Region2 {
        outer,
        holes: vec![],
    };
    let side = frame_at([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let s = crate::prism::extrude::extrude_region(&mut k, &side, &region, 0.0, 100.0).expect("s");
    let p = frame_at([0.0, 0.0, 12.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let deep = box_solid(&mut k, [20.0, 30.0, 3.0], [140.0, 36.0, 66.0]);
    assert!(passes_through(&k, &s, &deep, &p, TOL).expect("deep"));
    let shallow = box_solid(&mut k, [20.0, 30.0, 9.0], [140.0, 36.0, 66.0]);
    assert!(!passes_through(&k, &s, &shallow, &p, TOL).expect("shallow"));
    let thick = box_solid(&mut k, [10.0, 30.0, 3.0], [70.0, 36.0, 66.0]);
    assert!(!passes_through(&k, &s, &thick, &p, TOL).expect("thick"));
    // Flush with the thin part's bottom (T from z = 6): accepted.
    let flush = box_solid(&mut k, [20.0, 30.0, 6.0], [140.0, 36.0, 66.0]);
    assert!(!passes_through(&k, &s, &flush, &p, TOL).expect("flush"));
    assert!(passes_through(&k, &s, &deep, &p, 0.0).is_err());
}

/// The pair on F: Wall touching P (rise 66 − 6 = 60): a joint, Wall's y pair, 6 thick. No
/// contact: that contact is the skip. A 100 × 50 × 6 slab lying flat on P (strip x ∈ [0, 100],
/// y ∈ [0, 50], rise 6): the x pair is 100 thick, wider than the strip's 50 across it, out; the
/// y pair is 50 thick, not less than the rise: face to face, `not_square` with no angle. Wall
/// tilted 5° about x (its y faces' normals (0, −cos 5°, sin 5°) and (0, cos 5°, −sin 5°), the
/// first 85° from +z): no pair, `not_square` naming 85° (the first such face).
#[test]
fn the_pair_is_a_joint_a_skip_or_not_square() {
    let touch = Contact::Touch { delta: 0.0 };
    let wall_pair = || pair(touch, &wall(), Vec3::Z, 60.0, TOL, |m| Ok(strip(m)));
    let got = wall_pair().unwrap().unwrap();
    assert_eq!(got.faces.thickness, 6.0);
    assert_eq!(got.contact, touch);
    let far = Contact::NoContact { distance: 2.0 };
    assert_eq!(
        pair(far, &wall(), Vec3::Z, 60.0, TOL, |m| Ok(strip(m))).unwrap(),
        Err(PairSkip::Contact(far))
    );
    let slab = [
        plane(0.0, 0.0, 0.0, -1.0, 0.0, 0.0),
        plane(100.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        plane(0.0, 0.0, 0.0, 0.0, -1.0, 0.0),
        plane(0.0, 50.0, 0.0, 0.0, 1.0, 0.0),
    ];
    let slab_strip = |m: Vec3| {
        let levels = [(0.0, 0.0), (100.0, 0.0), (0.0, 50.0), (100.0, 50.0)]
            .map(|(x, y)| m.dot(Vec3::new(x, y, 6.0)));
        let lo = levels.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = levels.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Ok((lo, hi))
    };
    let flat = || pair(touch, &slab, Vec3::Z, 6.0, TOL, slab_strip);
    assert_eq!(flat().unwrap(), Err(PairSkip::NotSquare(None)));
    let tilt = 5.0_f64.to_radians();
    let tilted = [
        Plane::through(
            Vec3::new(0.0, 30.0, 6.0),
            Vec3::new(0.0, -tilt.cos(), tilt.sin()),
        )
        .unwrap(),
        Plane::through(
            Vec3::new(0.0, 36.0, 6.0),
            Vec3::new(0.0, tilt.cos(), -tilt.sin()),
        )
        .unwrap(),
    ];
    match pair(touch, &tilted, Vec3::Z, 60.0, TOL, |m| Ok(strip(m))).unwrap() {
        Err(PairSkip::NotSquare(Some(deg))) => assert!((deg - 85.0).abs() < 1e-9, "{deg}"),
        other => panic!("{other:?}"),
    }
}

/// Swapped [m10]: the slab flat on P is `not_square` face to face; the reverse pair (here Wall's
/// joint standing for it) is a joint: `swapped`. With no joint the other way: the first reason.
/// A forward joint does not ask the reverse; a contact skip is not swapped.
#[test]
fn a_skip_whose_reverse_is_a_joint_is_swapped() {
    let joint = || Ok(Ok(1));
    let face_to_face = || Ok(Err(PairSkip::NotSquare(None)));
    let none = || Ok(Err(PairSkip::NoPlateFaces(NoPlateFaces::NoPair)));
    assert_eq!(
        with_swap(face_to_face, joint).unwrap(),
        Err(PairSkip::Swapped)
    );
    assert_eq!(with_swap(none, joint).unwrap(), Err(PairSkip::Swapped));
    assert_eq!(
        with_swap(face_to_face, none).unwrap(),
        Err(PairSkip::NotSquare(None))
    );
    assert_eq!(with_swap(joint, || panic!("not asked")).unwrap(), Ok(1));
    let tilt = || Ok(Err::<i32, _>(PairSkip::NotSquare(Some(85.0))));
    assert_eq!(
        with_swap(tilt, || panic!("not asked")).unwrap(),
        Err(PairSkip::NotSquare(Some(85.0)))
    );
}

/// The pair read from the bodies on F: Bottom (0,0,0)–(160,100,6), P its top (z = 6, n = +z).
/// Wall standing on P: a touch at 0, Wall's y pair, 6 thick. Wall sunk 3 into Bottom (from
/// z = 3): an overlap, the same pair. Wall lifted to z = 10: no contact, 4 apart. A 100 × 50 × 6
/// slab lying flat on P: face to face, `not_square` with no angle. tol 0: refused.
#[test]
fn the_pair_read_from_the_bodies() {
    let mut k = crate::kernel::Kernel::default();
    let bottom = box_solid(&mut k, [0.0, 0.0, 0.0], [160.0, 100.0, 6.0]);
    let p = plane(0.0, 0.0, 6.0, 0.0, 0.0, 1.0);
    let wall = wall_solid(&mut k);
    let got = pair_at(&mut k, &wall, &bottom, p, TOL)
        .expect("wall")
        .expect("joint");
    assert_eq!(got.contact, Contact::Touch { delta: 0.0 });
    assert!((got.faces.thickness - 6.0).abs() < 1e-9, "{got:?}");
    assert_eq!(got.faces.b.normal, Vec3::Y);
    let sunk = box_solid(&mut k, [20.0, 30.0, 3.0], [140.0, 36.0, 66.0]);
    let got = pair_at(&mut k, &sunk, &bottom, p, TOL)
        .expect("sunk")
        .expect("joint");
    assert_eq!(got.contact, Contact::Overlap);
    assert!((got.faces.thickness - 6.0).abs() < 1e-9, "{got:?}");
    let lifted = box_solid(&mut k, [20.0, 30.0, 10.0], [140.0, 36.0, 66.0]);
    match pair_at(&mut k, &lifted, &bottom, p, TOL).expect("lifted") {
        Err(PairSkip::Contact(Contact::NoContact { distance })) => {
            assert!((distance - 4.0).abs() < 1e-2, "{distance}");
        }
        other => panic!("{other:?}"),
    }
    let slab = box_solid(&mut k, [0.0, 0.0, 6.0], [100.0, 50.0, 12.0]);
    assert_eq!(
        pair_at(&mut k, &slab, &bottom, p, TOL).expect("slab"),
        Err(PairSkip::NotSquare(None))
    );
    assert!(pair_at(&mut k, &wall, &bottom, p, 0.0).is_err());
}
