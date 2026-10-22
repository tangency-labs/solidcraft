//! The sweeps through SMLib ([`crate::smlib_sweeps`]): analytic volumes, and the history and
//! construction sources the builders' vocabulary gives each face.
#![cfg(feature = "smlib")]

use std::f64::consts::{PI, TAU};

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::FaceSource;
use crate::body::Body;
use crate::history::{FaceOrigin, History};
use crate::numbered::number_regions;
use crate::smlib_sweeps::{self, Draft, Piece};

fn region(outer: Loop2) -> Region2 {
    Region2 { outer, holes: Vec::new() }
}

fn pieces_of(r: &Region2) -> Vec<Piece> {
    let n = number_regions(std::slice::from_ref(r)).pop().unwrap().oriented();
    smlib_sweeps::pieces(n.loops())
}

/// Measured on the body's mesh: curved faces come out within about 1e-3.
fn volume(b: &Body) -> f64 {
    crate::measure(b).unwrap().volume
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    assert!((got - want).abs() <= rel * want.abs(), "{what}: {got} vs analytic {want}");
}

/// Each face's origins, sorted by their debug text (the faces' order is SMLib's).
fn origins(b: &Body) -> Vec<String> {
    let h: &History = b.history().expect("a history");
    let mut v: Vec<String> = h.faces.iter().map(|f| format!("{:?}", f.origins)).collect();
    v.sort();
    v
}

fn profile(k: usize) -> String {
    format!("{:?}", vec![FaceOrigin::Profile { segment: k }])
}

fn cap(end: bool) -> String {
    format!("{:?}", vec![FaceOrigin::Cap { end }])
}

#[test]
fn an_extrusion_names_its_caps_and_walls_and_keeps_their_sources() {
    let r = region(Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 6.0), Vec2::new(0.0, 6.0)]));
    let b = smlib_sweeps::extrude(&Plane::XY, &pieces_of(&r), Some(&r), 2.0, 5.0, 1.0, None).unwrap();
    close(volume(&b), 300.0, 1e-9, "prism");
    let mut want = vec![cap(false), cap(true), profile(0), profile(1), profile(2), profile(3)];
    want.sort();
    assert_eq!(origins(&b), want);
    let sources = b.face_sources().expect("sources");
    let caps = sources.iter().filter(|s| matches!(s, Some(FaceSource::ExtrudeCap { .. }))).count();
    let sides = sources.iter().filter(|s| matches!(s, Some(FaceSource::ExtrudeSide { plane: Some(_), .. }))).count();
    assert_eq!((caps, sides), (2, 4));
    // A drafted disc: a frustum, its wall the circle's.
    let disc = region(Loop2::circle(Vec2::ZERO, 5.0));
    let taper = -5f64.to_radians();
    let b = smlib_sweeps::extrude(&Plane::XY, &pieces_of(&disc), Some(&disc), 0.0, 4.0, 1.0, Some(Draft { taper })).unwrap();
    let r2 = 5.0 + 4.0 * taper.tan();
    close(volume(&b), PI * 4.0 / 3.0 * (25.0 + r2 * r2 + 5.0 * r2), 1e-3, "drafted disc");
    assert!(origins(&b).iter().filter(|o| o.contains("Profile")).count() >= 1);
}

#[test]
fn a_revolution_names_its_walls_by_segment_and_skips_the_axis() {
    // A half disc on the y axis of the xy plane: a sphere, every wall from the arc.
    let half = Loop2 {
        segs: vec![
            Seg2::Line { a: Vec2::new(0.0, -5.0), b: Vec2::new(0.0, 5.0) },
            Seg2::Arc { center: Vec2::ZERO, radius: 5.0, start: PI / 2.0, sweep: -PI },
        ],
    };
    let r = region(half);
    let b = smlib_sweeps::revolve(&Plane::XY, &pieces_of(&r), Vec3::ZERO, Vec3::Y, TAU).unwrap();
    close(volume(&b), 4.0 / 3.0 * PI * 125.0, 1e-3, "sphere (measured on a mesh)");
    assert!(origins(&b).iter().all(|o| *o == profile(1)), "{:?}", origins(&b));
    // A quarter turn of a square off the axis: two caps and four walls.
    let sq = region(Loop2::polygon(&[Vec2::new(6.0, 0.0), Vec2::new(8.0, 0.0), Vec2::new(8.0, 2.0), Vec2::new(6.0, 2.0)]));
    let b = smlib_sweeps::revolve(&Plane::XY, &pieces_of(&sq), Vec3::ZERO, Vec3::Y, PI / 2.0).unwrap();
    close(volume(&b), PI / 2.0 * 7.0 * 4.0, 1e-3, "quarter ring");
    let mut want = vec![cap(false), cap(true), profile(0), profile(1), profile(2), profile(3)];
    want.sort();
    assert_eq!(origins(&b), want);
}

#[test]
fn a_loft_and_a_screw_sweep_name_their_ends() {
    let square = |z: f64| {
        let r = region(Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]));
        (Plane::XY.offset(z), pieces_of(&r))
    };
    let b = smlib_sweeps::loft(&[square(0.0), square(8.0)], 0).unwrap();
    close(volume(&b), 800.0, 1e-6, "loft");
    let o = origins(&b);
    assert!(o.contains(&cap(false)) && o.contains(&cap(true)) && o.iter().filter(|x| x.contains("Profile")).count() == 4, "{o:?}");
    // A 0.4 mm square tooth at radius 4 in the xz plane, three turns of pitch 1 about z.
    let xz = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    let tooth = region(Loop2::polygon(&[Vec2::new(4.0, 0.0), Vec2::new(4.4, 0.0), Vec2::new(4.4, 0.4), Vec2::new(4.0, 0.4)]));
    let b = smlib_sweeps::screw(&xz, &pieces_of(&tooth), Vec3::ZERO, Vec3::Z, 1.0, 3.0).unwrap();
    close(volume(&b), 0.16 * TAU * 4.2 * 3.0, 1e-3, "screw");
    let o = origins(&b);
    assert!(o.contains(&cap(false)) && o.contains(&cap(true)), "{o:?}");
}

#[test]
fn a_sweep_keeps_a_profile_off_its_path_where_it_lies() {
    // A disc 5 mm beside a quarter-arc path of radius 20 about (-20, 0, 0): Pappus at 25.
    let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y };
    let disc = region(Loop2::circle(Vec2::new(5.0, 0.0), 2.0));
    let path = [crate::PathSeg::Arc { a: Vec3::ZERO, center: Vec3::new(-20.0, 0.0, 0.0), axis: -Vec3::Y, angle: PI / 2.0 }];
    let b = smlib_sweeps::sweep(&plane, &pieces_of(&disc), &path, false).unwrap();
    close(volume(&b), PI * 4.0 * (PI / 2.0 * 25.0), 1e-3, "bent tube");
}

fn numbered(r: &Region2) -> crate::numbered::NumberedRegion {
    number_regions(std::slice::from_ref(r)).pop().unwrap().oriented()
}

#[test]
fn a_sweep_twists_scales_and_follows_rails_through_stations() {
    let square = region(Loop2::polygon(&[Vec2::new(-5.0, -5.0), Vec2::new(5.0, -5.0), Vec2::new(5.0, 5.0), Vec2::new(-5.0, 5.0)]));
    let up = [crate::PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 40.0) }];
    // Turning about the section's centre keeps its area: V = 100 * 40, the caps and the four
    // walls named.
    let ctl = crate::SweepControl { twist: PI / 2.0, ..Default::default() };
    let b = smlib_sweeps::sweep_controlled(&Plane::XY, &numbered(&square), &up, &ctl).unwrap();
    close(volume(&b), 4000.0, 1e-3, "twisted square");
    let o = origins(&b);
    assert!(o.contains(&cap(false)) && o.contains(&cap(true)) && (0..4).all(|k| o.contains(&profile(k))), "{o:?}");
    // Scaled from 5 to 10 over 30, or between rails that widen as much: a cone frustum.
    let disc = region(Loop2::circle(Vec2::ZERO, 5.0));
    let frustum = |h: f64| h / 3.0 * PI * (25.0 + 100.0 + 50.0);
    let short = [crate::PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 30.0) }];
    let ctl = crate::SweepControl { scale: 2.0, ..Default::default() };
    close(volume(&smlib_sweeps::sweep_controlled(&Plane::XY, &numbered(&disc), &short, &ctl).unwrap()), frustum(30.0), 1e-3, "end scale");
    let rails = [vec![Vec3::new(5.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 30.0)], vec![Vec3::new(-5.0, 0.0, 0.0), Vec3::new(-10.0, 0.0, 30.0)]];
    for n in 1..=2 {
        let ctl = crate::SweepControl { rails: rails[..n].to_vec(), ..Default::default() };
        close(volume(&smlib_sweeps::sweep_controlled(&Plane::XY, &numbered(&disc), &short, &ctl).unwrap()), frustum(30.0), 1e-3, "rails");
    }
}

#[test]
fn a_twisted_sweep_round_a_closed_path_joins_its_ends() {
    // An 8 x 8 square round a circle of radius 30, a quarter turn of twist (onto itself): Pappus.
    let xz = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    let square = region(Loop2::polygon(&[Vec2::new(26.0, -4.0), Vec2::new(34.0, -4.0), Vec2::new(34.0, 4.0), Vec2::new(26.0, 4.0)]));
    let a = Vec3::new(30.0, 0.0, 0.0);
    let round = [
        crate::PathSeg::Arc { a, center: Vec3::ZERO, axis: Vec3::Z, angle: PI },
        crate::PathSeg::Arc { a: -a, center: Vec3::ZERO, axis: Vec3::Z, angle: PI },
    ];
    let ctl = crate::SweepControl { twist: PI / 2.0, ..Default::default() };
    let b = smlib_sweeps::sweep_controlled(&xz, &numbered(&square), &round, &ctl).unwrap();
    close(volume(&b), 64.0 * TAU * 30.0, 1e-3, "twisted ring");
    assert!(origins(&b).iter().all(|o| o.contains("Profile")), "{:?}", origins(&b));
    // A twist that does not bring the square back onto itself cannot close.
    let ctl = crate::SweepControl { twist: PI / 3.0, ..Default::default() };
    assert!(smlib_sweeps::sweep_controlled(&xz, &numbered(&square), &round, &ctl).is_err());
}
