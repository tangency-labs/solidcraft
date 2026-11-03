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
    let b = smlib_sweeps::extrude(
        &Plane::XY,
        &pieces_of(&disc),
        Some(&disc),
        0.0,
        4.0,
        1.0,
        Some(Draft { taper, foot: 0.0, corner: crate::DraftCorner::default() }),
    )
    .unwrap();
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

/// An 8 x 5 rectangle on the xy plane, centred on the path's start.
fn bar_section() -> Region2 {
    region(Loop2::polygon(&[Vec2::new(-4.0, -2.5), Vec2::new(4.0, -2.5), Vec2::new(4.0, 2.5), Vec2::new(-4.0, 2.5)]))
}

#[test]
fn a_sweep_round_a_corner_meets_in_a_mitre_and_names_every_wall() {
    // 30 up then 40 across: mitred, the outer side gains what the inner loses, V = A (l1 + l2).
    let path = [
        crate::PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 30.0) },
        crate::PathSeg::Line { a: Vec3::new(0.0, 0.0, 30.0), b: Vec3::new(40.0, 0.0, 30.0) },
    ];
    let b = smlib_sweeps::sweep(&Plane::XY, &pieces_of(&bar_section()), &path, false).unwrap();
    close(volume(&b), 2800.0, 1e-9, "mitred bar");
    // Each leg's four walls (the side walls in the bend's plane are one face round the corner,
    // flat as they are) from the four segments; the start and the end cap.
    let o = origins(&b);
    assert_eq!(o.iter().filter(|x| **x == cap(false)).count(), 1, "{o:?}");
    assert_eq!(o.iter().filter(|x| **x == cap(true)).count(), 1, "{o:?}");
    assert!(o.iter().all(|x| x.contains("Profile") || x.contains("Cap")), "{o:?}");
    for k in 0..4 {
        assert!(o.contains(&profile(k)), "segment {k}: {o:?}");
    }
    // Every wall of a straight leg is a plane.
    assert!((0..b.face_count()).all(|i| b.planar_face(i).is_some()), "{o:?}");
}

#[test]
fn a_sweep_round_a_closed_path_has_no_caps() {
    // A square path of side 40 round the origin, the section in the plane square to its first
    // side at its middle: four mitred legs closing on themselves. Each leg's centre line is 40,
    // so V = A 4 40 (the mitres trade what they add and take).
    let c = [Vec3::new(20.0, -20.0, 0.0), Vec3::new(20.0, 20.0, 0.0), Vec3::new(-20.0, 20.0, 0.0), Vec3::new(-20.0, -20.0, 0.0)];
    let start = Vec3::new(20.0, 0.0, 0.0);
    let mut path = vec![crate::PathSeg::Line { a: start, b: c[1] }];
    for i in 1..4 {
        path.push(crate::PathSeg::Line { a: c[i], b: c[(i + 1) % 4] });
    }
    path.push(crate::PathSeg::Line { a: c[0], b: start });
    let plane = Plane { origin: start, x: Vec3::X, y: Vec3::Z };
    let sq = region(Loop2::polygon(&[Vec2::new(-2.0, -1.0), Vec2::new(2.0, -1.0), Vec2::new(2.0, 1.0), Vec2::new(-2.0, 1.0)]));
    let b = smlib_sweeps::sweep(&plane, &pieces_of(&sq), &path, false).unwrap();
    close(volume(&b), 8.0 * 160.0, 1e-9, "square frame");
    let o = origins(&b);
    assert!(o.iter().all(|x| x.contains("Profile")), "no caps round a closed path: {o:?}");
}

#[test]
fn a_loft_through_holed_sections_keeps_the_hole_and_names_its_walls() {
    let ring = |z: f64| {
        let r = Region2 { outer: Loop2::circle(Vec2::ZERO, 20.0), holes: vec![Loop2::circle(Vec2::ZERO, 12.0)] };
        (Plane::XY.offset(z), r)
    };
    let b = crate::loft_regions(&[ring(0.0), ring(20.0)]).unwrap();
    close(volume(&b), PI * (400.0 - 144.0) * 20.0, 1e-3, "tube");
    let o = origins(&b);
    assert_eq!(b.face_count(), 4, "{o:?}");
    assert!(o.contains(&cap(false)) && o.contains(&cap(true)), "{o:?}");
    assert_eq!(o.iter().filter(|x| x.contains("Profile")).count(), 2, "outer wall and bore: {o:?}");
}

#[test]
fn a_loft_to_a_point_is_a_pyramid() {
    let sq = Loop2::polygon(&[Vec2::new(-10.0, -10.0), Vec2::new(10.0, -10.0), Vec2::new(10.0, 10.0), Vec2::new(-10.0, 10.0)]);
    let b = crate::loft_to_point(&Plane::XY, &sq, Vec3::new(0.0, 0.0, 15.0)).unwrap();
    close(volume(&b), 2000.0, 1e-9, "pyramid");
    let mut want = vec![cap(false), profile(0), profile(1), profile(2), profile(3)];
    want.sort();
    assert_eq!(origins(&b), want);
}

#[test]
fn a_drafted_extrusion_stands_on_a_straight_foot() {
    // A 10 x 10 square, 1 straight, then sloping out 5 degrees (taken from the base) up to 10:
    // the prism 100 and the frustum from 10 to 10 + 2 x 10 tan 5 over 9.
    let sq = region(Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]));
    let taper = 5f64.to_radians();
    let b = smlib_sweeps::extrude(
        &Plane::XY,
        &pieces_of(&sq),
        Some(&sq),
        0.0,
        10.0,
        1.0,
        Some(Draft { taper, foot: 1.0, corner: crate::DraftCorner::default() }),
    )
    .unwrap();
    let (a1, s2) = (100.0, 10.0 + 2.0 * 10.0 * taper.tan());
    let a2 = s2 * s2;
    close(volume(&b), 100.0 + 9.0 / 3.0 * (a1 + a2 + (a1 * a2).sqrt()), 1e-9, "footed draft");
    let o = origins(&b);
    assert!(o.contains(&cap(false)) && o.contains(&cap(true)), "{o:?}");
    // Each segment's foot and sloping wall.
    for k in 0..4 {
        assert_eq!(o.iter().filter(|x| **x == profile(k)).count(), 2, "segment {k}: {o:?}");
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

#[test]
fn a_sweep_follows_a_free_form_path_as_a_curve() {
    // A 4 x 4 square along a planar S-shaped cubic Bézier (curvature radius above 20): V = A L.
    let xz = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    let seg = Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(0.0, 20.0), p2: Vec2::new(20.0, 20.0), p3: Vec2::new(20.0, 40.0) };
    let path = [crate::PathSeg::Curve { plane: xz, seg }];
    let sq = region(Loop2::polygon(&[Vec2::new(-2.0, -2.0), Vec2::new(2.0, -2.0), Vec2::new(2.0, 2.0), Vec2::new(-2.0, 2.0)]));
    let b = smlib_sweeps::sweep(&Plane::XY, &pieces_of(&sq), &path, false).unwrap();
    close(volume(&b), 16.0 * 46.220575543, 1e-3, "square along a Bézier");
    assert_eq!(b.face_count(), 6, "four smooth walls and two caps");
    // Twisted along it, through stations placed by length along the curve: the area is kept.
    let ctl = crate::SweepControl { twist: PI / 2.0, ..Default::default() };
    let b = smlib_sweeps::sweep_controlled(&Plane::XY, &numbered(&sq), &path, &ctl).unwrap();
    close(volume(&b), 16.0 * 46.220575543, 1e-3, "twisted square along a Bézier");
}

/// A ring about the origin: `outer` radius with a hole of radius `bore`.
fn ring(outer: f64, bore: f64) -> Region2 {
    Region2 { outer: Loop2::circle(Vec2::ZERO, outer), holes: vec![Loop2::circle(Vec2::ZERO, bore)] }
}

/// The faces of `b` on a cylinder about the z axis of radius `r`.
fn z_cylinders(b: &Body, r: f64) -> usize {
    (0..b.face_count())
        .filter(|&f| match crate::face_surface(b, f) {
            Some(crate::FaceSurface::Cylinder { origin, axis, radius }) => {
                (radius - r).abs() <= 1e-9 && axis.cross(Vec3::Z).len() <= 1e-12 && origin.x.hypot(origin.y) <= 1e-9
            }
            _ => false,
        })
        .count()
}

#[test]
fn a_twisted_sweep_builds_circles_about_its_path_as_exact_cylinders() {
    // solidcore#2184: a tube r 2 / 1.5 extruded over z -2..3, joined with a tube r 2.5 / 1.5 swept
    // 1.25 up z with a twist. The twist turns each circle onto itself, so both of the swept tube's
    // walls are the exact cylinders, and its bore lies on the extruded tube's: one body,
    // pi (1.75 x 5 + 4 x 1.25 - 1.75 x 1.25) = 11.5625 pi. Skinned, the bore stayed within 4e-9
    // of the cylinder and the join failed at 16, 20 and 30 degrees.
    let a = crate::extrude(&Plane::XY, &[ring(2.0, 1.5)], -2.0, 3.0).unwrap().pop().unwrap();
    let up = [crate::PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 1.25) }];
    let want = PI * (1.75 * 5.0 + 4.0 * 1.25 - 1.75 * 1.25);
    for deg in [10.0f64, 16.0, 18.0, 20.0, 30.0] {
        let ctl = crate::SweepControl { twist: deg.to_radians(), ..Default::default() };
        let b = crate::sweep_with(&Plane::XY, &ring(2.5, 1.5), &up, &ctl).unwrap();
        assert_eq!((z_cylinders(&b, 1.5), z_cylinders(&b, 2.5)), (1, 1), "{deg} deg: the walls are cylinders");
        let mut want_b = vec![cap(false), cap(true), profile(0), profile(1)];
        want_b.sort();
        assert_eq!(origins(&b), want_b, "{deg} deg");
        let joined = crate::boolean(&a, &b, crate::BoolOp::Union).unwrap().unwrap();
        assert_eq!((joined.shell_count(), joined.validity()), (1, Vec::<String>::new()), "{deg} deg");
        close(volume(&joined), want, 1e-8, &format!("{deg} deg join"));
        assert!(z_cylinders(&joined, 1.5) >= 1, "{deg} deg: the bore is a cylinder");
    }
    // A hexagon (circumradius 2.5, across flats 4.33, clear of the extruded tube's r 2) with the
    // same bore, twisted 30 degrees: the hexagon's walls are skinned, the bore cut as the exact
    // cylinder and named by its circle. The join is A + B less A's ring over 1.25.
    let hex =
        Loop2::polygon(&(0..6).map(|i| Vec2::new(2.5 * (f64::from(i) * PI / 3.0).cos(), 2.5 * (f64::from(i) * PI / 3.0).sin())).collect::<Vec<_>>());
    let hexed = Region2 { outer: hex, holes: vec![Loop2::circle(Vec2::ZERO, 1.5)] };
    let ctl = crate::SweepControl { twist: 30f64.to_radians(), ..Default::default() };
    let b = crate::sweep_with(&Plane::XY, &hexed, &up, &ctl).unwrap();
    let hex_area = 1.5 * 3f64.sqrt() * 6.25;
    close(volume(&b), (hex_area - PI * 2.25) * 1.25, 1e-5, "twisted hexagon");
    assert_eq!(z_cylinders(&b, 1.5), 1, "the bore is a cylinder");
    let o = origins(&b);
    assert!(o.contains(&cap(false)) && o.contains(&cap(true)) && (0..7).all(|k| o.contains(&profile(k))), "{o:?}");
    let joined = crate::boolean(&a, &b, crate::BoolOp::Union).unwrap().unwrap();
    assert_eq!((joined.shell_count(), joined.validity()), (1, Vec::<String>::new()));
    close(volume(&joined), PI * 1.75 * 5.0 + (hex_area - PI * 2.25) * 1.25 - PI * 1.75 * 1.25, 1e-5, "hexagon join");
    // A circle off the path's line is skinned as before.
    let off = region(Loop2::circle(Vec2::new(0.5, 0.0), 1.0));
    let b = crate::sweep_with(&Plane::XY, &off, &up, &ctl).unwrap();
    assert!((0..b.face_count()).all(|f| !matches!(crate::face_surface(&b, f), Some(crate::FaceSurface::Cylinder { .. }))));
    close(volume(&b), PI * 1.25, 1e-6, "twisted circle off the axis");
}
