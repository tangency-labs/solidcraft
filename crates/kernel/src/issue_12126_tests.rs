//! Issue #12126: a sweep whose profile reaches past the centre of a bend in its path. SMLib
//! refused it (the arc's piece is a turn about an axis the profile crosses) with only "SMLib
//! failed (status 1025)", or round a closed path built a body passing through itself. The sweep
//! now cuts the profile along the bend's axis and turns each side on its own: the result is the
//! swept volume, filled where the sides of a closed path meet.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::FRAC_PI_2;

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::{PathSeg, sweep};

/// A square of side `side` on the plane across the path's start (the YZ plane at the origin).
fn square(side: f64) -> Region2 {
    let h = side / 2.0;
    Region2 { outer: Loop2::polygon(&[Vec2::new(-h, -h), Vec2::new(h, -h), Vec2::new(h, h), Vec2::new(-h, h)]), holes: vec![] }
}

fn across_start() -> Plane {
    Plane { origin: Vec3::ZERO, x: Vec3::Y, y: Vec3::Z }
}

/// A straight run along X, then a quarter turn of `radius` towards +Y.
fn line_then_bend(radius: f64) -> [PathSeg; 2] {
    [
        PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(20.0, 0.0, 0.0) },
        PathSeg::Arc { a: Vec3::new(20.0, 0.0, 0.0), center: Vec3::new(20.0, radius, 0.0), axis: Vec3::Z, angle: FRAC_PI_2 },
    ]
}

/// Measured on the body's mesh: curved faces come out within about 1e-3.
fn volume(b: &crate::Body) -> f64 {
    crate::measure(b).unwrap().volume
}

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= 1e-3 * want, "{what}: {got} vs analytic {want}");
}

#[test]
fn a_profile_past_the_centre_of_a_bend_sweeps_round_it() {
    // The square reaches 2 mm towards the bend's centre, which is 1.5 mm away. The run is a
    // 4 x 4 x 20 box; the bend adds a quarter turn of the 3.5 mm the square reaches beyond the
    // axis on the path's side (4 high). What it reaches past the axis turns back inside the box.
    let b = sweep(&across_start(), &square(4.0), &line_then_bend(1.5)).expect("the profile is cut along the bend's axis");
    close(volume(&b), 320.0 + FRAC_PI_2 / 2.0 * 3.5 * 3.5 * 4.0, "line then tight bend");
}

#[test]
fn a_profile_inside_the_bend_sweeps() {
    // The same square round a 3 mm bend stays clear of its centre.
    sweep(&across_start(), &square(4.0), &line_then_bend(3.0)).unwrap();
}

/// A closed square path of side `side` round the origin in the XY plane with corners rounded to
/// `r` (sharp for 0), from the middle of its +X side, travelling +Y (anticlockwise).
fn rounded_square(side: f64, r: f64) -> Vec<PathSeg> {
    let h = side / 2.0;
    let start = Vec3::new(h, 0.0, 0.0);
    let dirs = [Vec3::Y, Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0), Vec3::X];
    let centres = [Vec3::new(h - r, h - r, 0.0), Vec3::new(r - h, h - r, 0.0), Vec3::new(r - h, r - h, 0.0), Vec3::new(h - r, r - h, 0.0)];
    // The side a direction of travel has outward (to its right).
    let out = |d: Vec3| Vec3::new(d.y, -d.x, 0.0);
    let mut path = Vec::new();
    let mut at = start;
    for (i, (d, c)) in dirs.iter().zip(centres).enumerate() {
        let end = c + out(*d) * r;
        path.push(PathSeg::Line { a: at, b: end });
        if r > 0.0 {
            path.push(PathSeg::Arc { a: end, center: c, axis: Vec3::Z, angle: FRAC_PI_2 });
        }
        at = c + out(dirs[(i + 1) % 4]) * r;
    }
    path.push(PathSeg::Line { a: at, b: start });
    path
}

/// A bar 2 high on the plane across the +X side of a square path of side `side`, reaching
/// `inward` towards the square's centre and 1 outward.
fn bar_across(side: f64, inward: f64) -> (Plane, Region2) {
    let plane = Plane { origin: Vec3::new(side / 2.0, 0.0, 0.0), x: Vec3::new(-1.0, 0.0, 0.0), y: Vec3::Z };
    let r = Region2 {
        outer: Loop2::polygon(&[Vec2::new(-1.0, -1.0), Vec2::new(inward, -1.0), Vec2::new(inward, 1.0), Vec2::new(-1.0, 1.0)]),
        holes: vec![],
    };
    (plane, r)
}

/// The area of a square of side `side` with corners rounded to `r`.
fn rounded_square_area(side: f64, r: f64) -> f64 {
    side * side - (4.0 - std::f64::consts::PI) * r * r
}

fn origins(b: &crate::Body) -> Vec<String> {
    b.history().expect("a history").faces.iter().map(|f| format!("{:?}", f.origins)).collect()
}

#[test]
fn a_sweep_round_a_closed_path_whose_sides_meet_comes_out_filled() {
    // The bar reaches 25 mm in from each side of a 40 mm square with 3 mm corners: the sides meet,
    // and the result is the solid inside the path's 1 mm outward offset (a 42 mm square with 4 mm
    // corners), 2 high. Before, the corners passed through themselves (4131 mm³).
    let (plane, bar) = bar_across(40.0, 25.0);
    let b = sweep(&plane, &bar, &rounded_square(40.0, 3.0)).unwrap();
    close(volume(&b), 2.0 * rounded_square_area(42.0, 4.0), "filled rounded square");
    // One flat face on top and one underneath: the parts the sweep is built from leave no seams
    // across them.
    for z in [-1.0, 1.0] {
        let flat = (0..b.face_count())
            .filter(|i| {
                b.planar_face(*i).is_some_and(|pl| (pl.origin.z - z).abs() < 1e-9 && pl.normal().x.abs() < 1e-9 && pl.normal().y.abs() < 1e-9)
            })
            .count();
        assert_eq!(flat, 1, "faces at z = {z}");
    }
    let o = origins(&b);
    assert!(o.iter().all(|x| x.contains("Profile")), "no caps round a closed path: {o:?}");
}

#[test]
fn a_sweep_round_a_closed_path_past_its_corners_keeps_its_hole() {
    // 5 mm in from 3 mm corners: past their centres, though a 30 mm square hole stays (the
    // inward offset of the path has sharp corners).
    let (plane, bar) = bar_across(40.0, 5.0);
    let b = sweep(&plane, &bar, &rounded_square(40.0, 3.0)).unwrap();
    close(volume(&b), 2.0 * (rounded_square_area(42.0, 4.0) - 30.0 * 30.0), "frame past its corners");
}

#[test]
fn a_sweep_round_a_sharp_closed_path_whose_sides_meet_comes_out_filled() {
    let (plane, bar) = bar_across(40.0, 25.0);
    let b = sweep(&plane, &bar, &rounded_square(40.0, 0.0)).unwrap();
    close(volume(&b), 2.0 * 42.0 * 42.0, "filled square");
}

#[test]
fn a_sweep_clear_of_its_bends_is_swept_whole() {
    // 2 mm in from 3 mm corners: one sweep, with no parts to join.
    let (plane, bar) = bar_across(40.0, 2.0);
    crate::smlib_sweeps::UNION_LOAD.with(|l| l.set(0));
    let b = sweep(&plane, &bar, &rounded_square(40.0, 3.0)).unwrap();
    assert_eq!(crate::smlib_sweeps::UNION_LOAD.with(std::cell::Cell::get), 0, "a sweep clear of its bends takes no unions");
    close(volume(&b), 2.0 * (rounded_square_area(42.0, 4.0) - rounded_square_area(36.0, 1.0)), "frame");
}

#[test]
fn a_profile_reaching_far_from_where_it_starts_is_swept_whole_round_a_bend() {
    // A 12.5 mm circle drawn from its point nearest the path, 0.5 mm behind it, turned a quarter
    // round a 1.5 mm bend: the cut along the bend's axis has to reach the circle's far side, 24.5
    // mm out, not only where its one segment starts. The two sides turn into quarters on opposite
    // sides of the axis, so the volume is a quarter turn of the circle's first moment about it.
    let (cx, r, a) = (12.0, 12.5, 1.5);
    let circle = Region2 {
        outer: Loop2 {
            segs: vec![solidcore_record::Seg2::Arc {
                center: Vec2::new(cx, 0.0),
                radius: r,
                start: std::f64::consts::PI,
                sweep: std::f64::consts::TAU,
            }],
        },
        holes: vec![],
    };
    let bend = [PathSeg::Arc { a: Vec3::ZERO, center: Vec3::new(0.0, a, 0.0), axis: Vec3::Z, angle: FRAC_PI_2 }];
    let b = sweep(&across_start(), &circle, &bend).expect("the profile is cut along the bend's axis");
    // ∫ |x - a| dA over the disc, by Simpson's rule across its width.
    let n = 20_000;
    let h = 2.0 * r / f64::from(n);
    let moment: f64 = (0..=n)
        .map(|i| {
            let x = cx - r + h * f64::from(i);
            let w = if i == 0 || i == n {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            w * (x - a).abs() * 2.0 * (r * r - (x - cx) * (x - cx)).max(0.0).sqrt()
        })
        .sum::<f64>()
        * h
        / 3.0;
    close(volume(&b), FRAC_PI_2 * moment, "circle round a tight bend");
}

#[test]
fn a_profile_touching_the_centre_of_a_bend_is_swept_whole() {
    // The square reaches 2 mm towards a 2 mm bend's centre: it touches the axis, crosses nothing,
    // and SMLib sweeps it in one piece as before (the reference case 50-sweep-tight-bend).
    crate::smlib_sweeps::UNION_LOAD.with(|l| l.set(0));
    let b = sweep(&across_start(), &square(4.0), &line_then_bend(2.0)).unwrap();
    assert_eq!(crate::smlib_sweeps::UNION_LOAD.with(std::cell::Cell::get), 0, "a profile touching the axis takes no unions");
    close(volume(&b), 320.0 + FRAC_PI_2 / 2.0 * 4.0 * 4.0 * 4.0, "line then bend as tight as the profile");
}
