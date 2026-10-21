//! The creation builders' histories: every face's origin checked against the geometry the
//! builder made it from.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::body::Body;
use crate::build::PathSeg;
use crate::history::BodyHistory;
use crate::history::{Change, EdgeOrigin, FaceOrigin, FaceProv, History, Side};

const P: fn(usize) -> FaceOrigin = |segment| FaceOrigin::Profile { segment };
const START: FaceOrigin = FaceOrigin::Cap { end: false };
const END: FaceOrigin = FaceOrigin::Cap { end: true };

/// The body's history, checked against it; a body made from a sketch has generated faces only
/// and its edges, all new, separate the faces the body's edge order says.
fn sketch_made(b: &Body) -> &History {
    let h = b.history().expect("the builder reports its history");
    h.check(b, &[]).unwrap();
    assert!(h.faces.iter().all(|f| f.change == Change::Generated && !f.origins.is_empty()));
    assert!(h.edges.iter().all(|e| e.origins.is_empty()));
    assert_eq!(h.edges.iter().map(|e| e.faces.clone()).collect::<Vec<_>>(), crate::history::edge_faces(&b.solid));
    h
}

/// Each face's origins.
fn origins(b: &Body) -> Vec<Vec<FaceOrigin>> {
    sketch_made(b).faces.iter().map(|f| f.origins.clone()).collect()
}

/// Mesh vertices of each face.
fn points(b: &Body) -> Vec<Vec<Vec3>> {
    mesh_points(b).0
}

/// Mesh vertices of each face, and its triangles' centres (off a curved face by up to the
/// mesh's tolerance).
fn mesh_points(b: &Body) -> (Vec<Vec<Vec3>>, Vec<Vec<Vec3>>) {
    let m = b.tessellate(b.size() * 1e-3).unwrap();
    let (mut corners, mut centres) = (vec![Vec::new(); b.face_count()], vec![Vec::new(); b.face_count()]);
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let c = t.map(|i| m.positions[i as usize]);
        corners[*f as usize].extend(c);
        centres[*f as usize].push((c[0] + c[1] + c[2]) * (1.0 / 3.0));
    }
    (corners, centres)
}

/// The segments a face lies over: its vertices on them within `tol`, its triangles' centres
/// near them (`to_2d` takes a point into the profile's plane).
fn swept_from(b: &Body, loops: &[&Loop2], to_2d: impl Fn(Vec3) -> Vec2, tol: f64) -> Vec<Vec<usize>> {
    let (corners, centres) = mesh_points(b);
    let flat = |pts: &Vec<Vec3>| pts.iter().map(|p| to_2d(*p)).collect::<Vec<_>>();
    corners
        .iter()
        .zip(&centres)
        .map(|(v, c)| {
            let near = under(loops, &flat(c), b.size() * 3e-3);
            under(loops, &flat(v), tol).into_iter().filter(|k| near.contains(k)).collect()
        })
        .collect()
}

/// The segments (of all `loops`, numbered in order) every point lies on.
fn under(loops: &[&Loop2], pts: &[Vec2], tol: f64) -> Vec<usize> {
    let segs: Vec<Seg2> = loops.iter().flat_map(|l| l.segs.iter().copied()).collect();
    (0..segs.len()).filter(|k| !pts.is_empty() && pts.iter().all(|q| segs[*k].point_at(segs[*k].closest_param(*q)).dist(*q) < tol)).collect()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
    Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)])
}

fn rotated(l: &Loop2, k: usize) -> Loop2 {
    let mut segs = l.segs.clone();
    segs.rotate_left(k);
    Loop2 { segs }
}

/// A body swept from `r`: each face from the profile segments it lies over (`to_2d` takes a
/// point back into the profile's plane, within `tol`), or else the cap `cap` says its points
/// lie on (if any).
fn expected_swept(
    b: &Body,
    r: &Region2,
    to_2d: impl Fn(Vec3) -> Vec2,
    tol: f64,
    cap: impl Fn(&[Vec3]) -> Option<FaceOrigin>,
) -> Vec<Vec<FaceOrigin>> {
    let loops: Vec<&Loop2> = std::iter::once(&r.outer).chain(&r.holes).collect();
    let swept = swept_from(b, &loops, to_2d, tol);
    points(b)
        .iter()
        .zip(swept)
        .map(|(pts, segs)| if segs.is_empty() { cap(pts).into_iter().collect() } else { segs.into_iter().map(P).collect() })
        .collect()
}

/// An extrusion (`h` up from `plane`): its sides by the profile segment under them, its caps
/// by height.
fn expected_extrusion(b: &Body, plane: &Plane, r: &Region2, h: f64) -> Vec<Vec<FaceOrigin>> {
    let at = |pts: &[Vec3], z: f64| pts.iter().all(|p| (plane.height(*p) - z).abs() < 1e-9);
    expected_swept(
        b,
        r,
        |p| plane.to_local(p),
        1e-7,
        |pts| {
            if at(pts, 0.0) {
                Some(START)
            } else if at(pts, h) {
                Some(END)
            } else {
                None
            }
        },
    )
}

/// Side k comes from segment k whatever order and direction the loops are given in.
#[test]
fn extrude_sides_follow_profile_segments_in_any_order() {
    let outer = rect(0.0, 0.0, 40.0, 30.0);
    let hole = rect(10.0, 10.0, 20.0, 20.0).reversed();
    let variants = [
        Region2 { outer: outer.clone(), holes: vec![hole.clone()] },
        Region2 { outer: outer.reversed(), holes: vec![hole.clone()] },
        Region2 { outer: rotated(&outer, 2), holes: vec![rotated(&hole.reversed(), 1)] },
        Region2 { outer: rotated(&outer.reversed(), 3), holes: vec![rotated(&hole, 3)] },
    ];
    for r in &variants {
        let b = crate::extrude(&Plane::XY, std::slice::from_ref(r), 0.0, 10.0).unwrap().pop().unwrap();
        let got = origins(&b);
        assert_eq!(got, expected_extrusion(&b, &Plane::XY, r, 10.0));
        // One face per segment, two caps.
        let mut all: Vec<FaceOrigin> = got.into_iter().flatten().collect();
        all.sort();
        assert_eq!(all, [(0..8).map(P).collect::<Vec<_>>(), vec![START, END]].concat());
        // The certificates are produced as before.
        assert!(b.face_sources().unwrap().iter().all(Option::is_some));
    }
    // The +x side is whichever segment of the given loop runs along x = 40.
    let r = &variants[2];
    let b = crate::extrude(&Plane::XY, std::slice::from_ref(r), 0.0, 10.0).unwrap().pop().unwrap();
    let k = r.outer.segs.iter().position(|s| s.start().x == 40.0 && s.end().x == 40.0).unwrap();
    let side = b.faces(1e-3).unwrap().iter().position(|f| f.plane_normal.is_some_and(|n| n.dist(Vec3::X) < 1e-9)).unwrap();
    assert_eq!(origins(&b)[side], [P(k)]);
}

/// A big arc is built as two edges: both faces come from it.
#[test]
fn extrude_a_big_arc_gives_two_faces_of_one_segment() {
    let (a, b) = (Vec2::new(10.0 * (0.3f64).cos(), -10.0 * (0.3f64).sin()), Vec2::new(10.0 * (0.3f64).cos(), 10.0 * (0.3f64).sin()));
    let arc = Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: 0.3, sweep: TAU - 0.6 };
    let r = Region2 { outer: Loop2 { segs: vec![arc, Seg2::Line { a, b }] }, holes: vec![] };
    let body = crate::extrude(&Plane::XY, std::slice::from_ref(&r), 0.0, 5.0).unwrap().pop().unwrap();
    let got = origins(&body);
    assert_eq!(got, expected_extrusion(&body, &Plane::XY, &r, 5.0));
    assert_eq!(got.iter().filter(|o| **o == [P(0)]).count(), 2);
    assert_eq!(got.iter().filter(|o| **o == [P(1)]).count(), 1);
}

#[test]
fn primitives_report_their_construction() {
    // Box: the rectangle's sides (−y, +x, +y, −x) and the caps (−z, +z).
    let b = crate::box_solid(Vec3::ZERO, Vec3::new(4.0, 3.0, 2.0)).unwrap();
    let got = origins(&b);
    for (f, o) in b.faces(1e-3).unwrap().iter().zip(&got) {
        let n = f.plane_normal.unwrap();
        let want = [(-Vec3::Y, P(0)), (Vec3::X, P(1)), (Vec3::Y, P(2)), (-Vec3::X, P(3)), (-Vec3::Z, START), (Vec3::Z, END)]
            .into_iter()
            .find(|(d, _)| d.dist(n) < 1e-9)
            .unwrap()
            .1;
        assert_eq!(o, &[want]);
    }
    assert!(b.face_sources().unwrap().iter().all(|s| matches!(s, Some(crate::FaceSource::Box { .. }))));
    // Cylinder: a circle of two arcs, extruded.
    let c = crate::cylinder(Vec3::ZERO, Vec3::Z, 5.0, 8.0).unwrap();
    let r = Region2 { outer: Loop2::circle_from(Vec2::ZERO, 5.0, crate::build::SEAM_ANGLE), holes: vec![] };
    let frame = Plane::from_normal(Vec3::ZERO, Vec3::Z).unwrap();
    let want = expected_extrusion(&c, &frame, &r, 8.0);
    assert_eq!(origins(&c), want);
    // Sphere: six patches, all from the arc of the half disc it revolves.
    let s = crate::sphere(Vec3::ZERO, 5.0).unwrap();
    assert_eq!(origins(&s), vec![vec![P(0)]; 6]);
    // Torus: a circle re-split for its seam, turned in three: each face from both arcs.
    let t = crate::torus(Vec3::ZERO, 10.0, 2.0).unwrap();
    assert_eq!(origins(&t), vec![vec![P(0), P(1)]; 6]);
}

/// The plane the revolve tests sketch in (x across, y up the world z axis, the revolve axis).
const SIDE: Plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };

/// A revolution's faces by the profile segment under each point turned back into the sketch,
/// the angle caps by where they lie (the start in the sketch plane).
fn expected_revolution(b: &Body, r: &Region2) -> Vec<Vec<FaceOrigin>> {
    expected_swept(
        b,
        r,
        |p| Vec2::new(p.x.hypot(p.y), p.z),
        1e-6,
        |pts| Some(if pts.iter().all(|p| p.y.abs() < 1e-9 && p.x > -1e-9) { START } else { END }),
    )
}

#[test]
fn revolve_partial_and_full() {
    let r = Region2 { outer: rect(10.0, 0.0, 20.0, 5.0), holes: vec![rect(12.0, 1.0, 14.0, 2.0).reversed()] };
    // Under half a turn truck sweeps in two parts, beyond it in three; a full turn has no caps,
    // and its planar faces (from the segments square to the axis) are whole (#726), as a partial
    // turn's are when it is final (`revolve_whole`).
    let segs: Vec<Seg2> = r.outer.segs.iter().chain(r.holes.iter().flat_map(|h| &h.segs)).cloned().collect();
    for whole in [false, true] {
        for (angle, parts, caps) in [(FRAC_PI_2, 2, 2), (1.5 * PI, 3, 2), (TAU, 3, 0)] {
            let revolve = if whole { crate::revolve_whole } else { crate::revolve };
            let b = revolve(&SIDE, std::slice::from_ref(&r), Vec2::ZERO, Vec2::Y, angle).unwrap().pop().unwrap();
            let got = origins(&b);
            assert_eq!(got, expected_revolution(&b, &r), "angle {angle}, whole {whole}");
            for (k, s) in segs.iter().enumerate() {
                let square = matches!(s, Seg2::Line { a, b } if (a.y - b.y).abs() < 1e-12);
                let want = if (angle == TAU || whole) && square { 1 } else { parts };
                assert_eq!(got.iter().filter(|o| **o == [P(k)]).count(), want, "segment {k} at angle {angle}, whole {whole}");
            }
            assert_eq!(got.iter().filter(|o| **o == [START] || **o == [END]).count(), caps);
        }
    }
}

/// Loops given clockwise, a hole that is a full circle of three arcs: extrude numbers every
/// face by the segment as given; revolve re-seams the circle into two half circles, each from
/// every given arc it runs over.
#[test]
fn clockwise_loops_and_a_reseamed_circle_keep_their_numbers() {
    let (c, rad) = (Vec2::new(20.0, 10.0), 4.0);
    let arcs = [(0.3, 2.0), (2.3, 2.5), (4.8, TAU - 4.5)];
    let ccw = Loop2 { segs: arcs.iter().map(|&(start, sweep)| Seg2::Arc { center: c, radius: rad, start, sweep }).collect() };
    // Clockwise: segment 4 is the third arc above, 5 the second, 6 the first.
    let r = Region2 { outer: rect(10.0, 0.0, 30.0, 20.0).reversed(), holes: vec![ccw.reversed()] };
    let b = crate::extrude(&Plane::XY, std::slice::from_ref(&r), 0.0, 10.0).unwrap().pop().unwrap();
    let got = origins(&b);
    assert_eq!(got, expected_extrusion(&b, &Plane::XY, &r, 10.0));
    for k in 0..7 {
        assert_eq!(got.iter().filter(|o| **o == [P(k)]).count(), 1, "segment {k}");
    }
    // Revolved, the circle restarts at 0.4142 rad: the half from there runs over the first two
    // arcs (6, 5), the other half over all three (it ends past the first arc's start).
    let first_half = |q: Vec2| (q - c).angle().rem_euclid(TAU);
    for angle in [FRAC_PI_2, TAU] {
        let b = crate::revolve(&SIDE, std::slice::from_ref(&r), Vec2::ZERO, Vec2::Y, angle).unwrap().pop().unwrap();
        let got = origins(&b);
        let want = expected_revolution(&b, &r);
        let (_, centres) = mesh_points(&b);
        let mut halves = [0, 0];
        for ((g, w), centres) in got.iter().zip(&want).zip(&centres) {
            let flat: Vec<Vec2> = centres.iter().map(|p| Vec2::new(p.x.hypot(p.y), p.z)).collect();
            if flat.iter().all(|q| (q.dist(c) - rad).abs() < 0.1) {
                let first = flat.iter().all(|q| (0.4142 - 1e-3..=0.4142 + PI + 1e-3).contains(&first_half(*q)));
                assert_eq!(g, &if first { vec![P(5), P(6)] } else { vec![P(4), P(5), P(6)] }, "angle {angle}");
                halves[usize::from(!first)] += 1;
            } else {
                assert_eq!(g, w, "angle {angle}");
            }
        }
        // Truck turns each half in as many parts as each rectangle side.
        let parts = got.iter().filter(|o| **o == [P(0)]).count();
        assert_eq!(halves, [parts, parts], "angle {angle}");
    }
}

#[test]
fn revolve_with_an_edge_on_the_axis() {
    // Segment 3 runs down the axis: it makes no face.
    let r = Region2 { outer: rect(0.0, 0.0, 10.0, 5.0), holes: vec![] };
    let partial = crate::revolve(&SIDE, std::slice::from_ref(&r), Vec2::ZERO, Vec2::Y, FRAC_PI_2).unwrap().pop().unwrap();
    let got = origins(&partial);
    assert_eq!(got, expected_revolution(&partial, &r));
    assert_eq!(got.len(), 5);
    // A full turn: the radial segments 0 and 2 sweep discs, segment 1 the cylinder (three parts).
    let full = crate::revolve(&SIDE, std::slice::from_ref(&r), Vec2::ZERO, Vec2::Y, TAU).unwrap().pop().unwrap();
    let got = origins(&full);
    assert_eq!(got, expected_revolution(&full, &r));
    assert_eq!(got.iter().filter(|o| **o == [P(1)]).count(), 3);
    assert_eq!(got.iter().filter(|o| **o == [P(0)] || **o == [P(2)]).count(), 2);
    // A cone: the slanted segment ends on the axis (the apex), the base disc.
    let tri = Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 8.0)]), holes: vec![] };
    let cone = crate::revolve(&SIDE, std::slice::from_ref(&tri), Vec2::ZERO, Vec2::Y, TAU).unwrap().pop().unwrap();
    assert_eq!(origins(&cone), expected_revolution(&cone, &tri));
    // The same cone from a loop that starts elsewhere and runs the other way.
    let turned = Region2 { outer: rotated(&tri.outer.reversed(), 1), holes: vec![] };
    let cone = crate::revolve(&SIDE, std::slice::from_ref(&turned), Vec2::ZERO, Vec2::Y, TAU).unwrap().pop().unwrap();
    assert_eq!(origins(&cone), expected_revolution(&cone, &turned));
    // A half disc: a sphere, every patch from the arc (segment 1 here).
    let half = Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(0.0, 5.0), b: Vec2::new(0.0, -5.0) },
                Seg2::Arc { center: Vec2::ZERO, radius: 5.0, start: -FRAC_PI_2, sweep: PI },
            ],
        },
        holes: vec![],
    };
    let ball = crate::revolve(&SIDE, std::slice::from_ref(&half), Vec2::ZERO, Vec2::Y, TAU).unwrap().pop().unwrap();
    assert_eq!(origins(&ball), vec![vec![P(1)]; 6]);
}

#[test]
fn tapered_extrusion() {
    let r = Region2 { outer: rotated(&rect(0.0, 0.0, 20.0, 10.0).reversed(), 1), holes: vec![] };
    let b = crate::extrude_tapered(&Plane::XY, &r, 10.0, 1.0, 0.1).unwrap();
    let want: Vec<Vec<FaceOrigin>> = points(&b)
        .iter()
        .map(|pts| {
            let foot: Vec<Vec2> = pts.iter().filter(|p| p.z.abs() < 1e-9).map(|p| Vec2::new(p.x, p.y)).collect();
            if foot.len() == pts.len() {
                vec![START]
            } else if foot.is_empty() {
                vec![END]
            } else {
                under(&[&r.outer], &foot, 1e-7).into_iter().map(P).collect()
            }
        })
        .collect();
    assert_eq!(origins(&b), want);
}

/// Each loft face from the section segments its edges in each section's plane lie on.
fn expected_loft(b: &Body, sections: &[(Plane, Loop2)]) -> Vec<Vec<FaceOrigin>> {
    let bases: Vec<usize> = sections.iter().scan(0, |acc, (_, l)| Some(std::mem::replace(acc, *acc + l.segs.len()))).collect();
    points(b)
        .iter()
        .map(|pts| {
            let mut out = Vec::new();
            let mut whole = None;
            for (i, (p, l)) in sections.iter().enumerate() {
                let near: Vec<Vec2> = pts.iter().filter(|q| p.height(**q).abs() < 1e-9).map(|q| p.to_local(*q)).collect();
                if near.len() == pts.len() {
                    whole = Some(i);
                }
                if near.len() >= 2 {
                    out.extend(under(&[l], &near, 1e-3).into_iter().map(|k| P(bases[i] + k)));
                }
            }
            match whole {
                Some(0) => vec![START],
                Some(i) if i + 1 == sections.len() => vec![END],
                _ => {
                    out.sort();
                    out.dedup();
                    out
                }
            }
        })
        .collect()
}

#[test]
fn loft_sections() {
    let at = |z: f64| Plane { origin: Vec3::new(0.0, 0.0, z), ..Plane::XY };
    let low = (at(0.0), rect(-10.0, -10.0, 10.0, 10.0));
    let high = (at(10.0), rotated(&rect(-5.0, -5.0, 5.0, 5.0).reversed(), 2));
    // Two sections, either way round: ruled faces, each between a segment of each section.
    for sections in [vec![low.clone(), high.clone()], vec![high.clone(), low.clone()]] {
        let b = crate::loft(&sections).unwrap();
        let got = origins(&b);
        assert_eq!(got, expected_loft(&b, &sections));
        assert!(got.iter().filter(|o| o.len() == 2).all(|o| o[0] < P(4) && o[1] >= P(4)));
    }
    // Three sections: smooth faces through all of them (the middle section leaves no edge).
    let mid = (at(5.0), rect(-7.0, -7.0, 7.0, 7.0));
    let sections = [low.clone(), mid, high.clone()];
    let b = crate::loft(&sections).unwrap();
    for o in origins(&b).iter().filter(|o| o.len() > 1) {
        assert_eq!(o.len(), 3, "{o:?}");
        assert!(o[0] < P(4) && o[1] >= P(4) && o[1] < P(8) && o[2] >= P(8));
    }
    let got = origins(&b);
    let want = expected_loft(&b, &sections);
    // (The smooth faces' middle-section segment is checked above.)
    for (g, w) in got.iter().zip(&want) {
        assert!(w.iter().all(|x| g.contains(x)), "{g:?} vs {w:?}");
    }
    // A circle and a square: the circle is re-split at the square's corners, each piece from
    // the arcs it runs over.
    let round = (at(10.0), Loop2::circle(Vec2::ZERO, 6.0));
    let b = crate::loft(&[low, round]).unwrap();
    let got = origins(&b);
    assert_eq!(got.iter().filter(|o| o.len() > 1).count(), 4);
    for o in got.iter().filter(|o| o.len() > 1) {
        assert!(o[0] < P(4) && o[1..].iter().all(|x| *x == P(4) || *x == P(5)), "{o:?}");
    }
    // To a point: the base is the start cap, each triangle from its base segment.
    let base = rotated(&rect(-10.0, -10.0, 10.0, 10.0).reversed(), 1);
    let b = crate::loft_to_point(&Plane::XY, &base, Vec3::new(0.0, 0.0, 15.0)).unwrap();
    let want = points(&b)
        .iter()
        .map(|pts| {
            let foot: Vec<Vec2> = pts.iter().filter(|p| p.z.abs() < 1e-9).map(|p| Vec2::new(p.x, p.y)).collect();
            if foot.len() == pts.len() { vec![START] } else { under(&[&base], &foot, 1e-7).into_iter().map(P).collect() }
        })
        .collect::<Vec<_>>();
    assert_eq!(origins(&b), want);
}

/// Faces of a sweep: the start cap in the profile's plane; each segment's faces run the whole
/// path, the first one standing on that segment.
fn check_sweep(b: &Body, plane: &Plane, r: &Region2, faces_per_segment: Option<usize>) {
    let loops: Vec<&Loop2> = std::iter::once(&r.outer).chain(&r.holes).collect();
    let n: usize = loops.iter().map(|l| l.segs.len()).sum();
    let got = origins(b);
    let pts = points(b);
    assert_eq!(got.iter().filter(|o| **o == [START]).count(), 1);
    assert_eq!(got.iter().filter(|o| **o == [END]).count(), 1);
    for (o, p) in got.iter().zip(&pts) {
        let in_plane: Vec<Vec2> = p.iter().filter(|q| plane.height(**q).abs() < 1e-6).map(|q| plane.to_local(*q)).collect();
        // (Near the profile: a coil crosses its plane again further round.)
        let foot: Vec<Vec2> = in_plane
            .iter()
            .copied()
            .filter(|q| loops.iter().flat_map(|l| &l.segs).any(|s| s.point_at(s.closest_param(*q)).dist(*q) < 1e-3))
            .collect();
        if *o == [START] {
            assert_eq!(in_plane.len(), p.len());
        } else if let [FaceOrigin::Profile { segment }] = o.as_slice()
            && foot.len() >= 2
        {
            assert_eq!(under(&loops, &foot, 1e-3), [*segment], "{foot:?}");
        }
    }
    let counts: Vec<usize> = (0..n).map(|k| got.iter().filter(|o| **o == [P(k)]).count()).collect();
    match faces_per_segment {
        Some(c) => assert!(counts.iter().all(|x| *x == c), "{counts:?}"),
        None => assert!(counts.windows(2).all(|w| w[0] == w[1] && w[0] > 0), "{counts:?}"),
    }
}

#[test]
fn sweeps_along_paths() {
    let r = Region2 { outer: rotated(&rect(-2.0, -2.0, 2.0, 2.0).reversed(), 1), holes: vec![] };
    // A line then a quarter turn (two parts): three faces per segment.
    let path = [
        PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 10.0) },
        PathSeg::Arc { a: Vec3::new(0.0, 0.0, 10.0), center: Vec3::new(20.0, 0.0, 10.0), axis: Vec3::Y, angle: FRAC_PI_2 },
    ];
    let b = crate::sweep(&Plane::XY, &r, &path).unwrap();
    check_sweep(&b, &Plane::XY, &r, Some(3));
    // Along a smooth 3D path: one wall per segment.
    let curve: Vec<Vec3> = (0..=40)
        .map(|i| {
            let t = i as f64 / 40.0 * FRAC_PI_2;
            Vec3::new(30.0 * t.sin(), 30.0 * (1.0 - t.cos()), 10.0 * t)
        })
        .collect();
    let plane = Plane::from_normal(curve[0], (curve[1] - curve[0]).normalized().unwrap()).unwrap();
    let circle = Region2 { outer: Loop2::circle(Vec2::ZERO, 2.0), holes: vec![] };
    let b = crate::sweep_path(&plane, &circle, &curve).unwrap();
    check_sweep(&b, &plane, &circle, Some(1));
    // A path with a corner, a holed profile: ruled walls station to station.
    let holed = Region2 { outer: rect(-3.0, -3.0, 3.0, 3.0), holes: vec![rect(-1.0, -1.0, 1.0, 1.0).reversed()] };
    let b = crate::sweep_path(&Plane::XY, &holed, &[Vec3::ZERO, Vec3::new(0.0, 0.0, 20.0), Vec3::new(20.0, 0.0, 20.0)]).unwrap();
    check_sweep(&b, &Plane::XY, &holed, Some(2));
    // A coil: a smooth wall per segment.
    let side = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    let ring = Region2 { outer: Loop2::circle(Vec2::new(20.0, 0.0), 2.0), holes: vec![] };
    let b = crate::sweep_helix(&side, &ring, Vec3::ZERO, Vec3::Z, 8.0, 3.0).unwrap();
    check_sweep(&b, &side, &ring, Some(1));
}

/// Kept faces are their origins, unchanged; each edge an operand still owns is that edge.
fn check_kept(result: &Body, input: &Body) {
    let h = result.history().unwrap();
    h.check(result, &[input]).unwrap();
    let (rf, inf) = (result.faces(1e-3).unwrap(), input.faces(1e-3).unwrap());
    for (f, p) in rf.iter().zip(&h.faces) {
        if let (Change::Kept, [FaceOrigin::Face { operand: 0, face }]) = (p.change, p.origins.as_slice()) {
            assert!(f.centroid.dist(inf[*face].centroid) < 1e-6, "face {}", f.index);
        }
    }
    let (re, ie) = (result.edges(1e-3).unwrap(), input.edges(1e-3).unwrap());
    let mut owned = 0;
    for (e, p) in re.iter().zip(&h.edges) {
        for o in &p.origins {
            let crate::history::EdgeOrigin::Edge { operand: 0, edge } = o else { panic!("{o:?}") };
            assert!(e.mid.dist(ie[*edge].mid) < 1e-6, "edge {}", e.index);
            owned += 1;
        }
    }
    assert!(owned > 0);
}

/// The input edge (among `edges`) nearest the face: the edge a wall stands on.
fn nearest_edge(result: &Body, face: usize, input: &Body, edges: &[usize]) -> usize {
    let c = result.faces(1e-3).unwrap()[face].centroid;
    let mids = input.edges(1e-3).unwrap();
    *edges.iter().min_by(|a, b| c.dist(mids[**a].mid).total_cmp(&c.dist(mids[**b].mid))).unwrap()
}

#[test]
fn thicken_reports_copies_offsets_and_walls() {
    let c = crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
    let half = crate::copy_faces(&c, &[Vec3::new(7.0, 7.0, 10.0)]).unwrap();
    let t = crate::thicken(&half, 1.0, false).unwrap();
    check_kept(&t, &half);
    let h = t.history().unwrap();
    assert_eq!(h.faces[0].origins, [FaceOrigin::Face { operand: 0, face: 0 }]);
    assert_eq!(h.faces[1].origins, [FaceOrigin::Offset { operand: 0, face: 0 }]);
    let free: Vec<usize> = (0..half.edge_count()).collect();
    for (i, f) in h.faces.iter().enumerate().skip(2) {
        let [FaceOrigin::Edge { operand: 0, edge }] = f.origins.as_slice() else { panic!("{f:?}") };
        assert_eq!(*edge, nearest_edge(&t, i, &half, &free));
    }
    assert_eq!(h.faces.len(), 2 + half.edge_count());
}

#[test]
fn raised_band_reports_kept_trimmed_and_new_faces() {
    let b = crate::box_solid(Vec3::new(-40.0, -30.0, 0.0), Vec3::new(40.0, 30.0, 30.0)).unwrap();
    let sh = crate::shell(&b, &[Vec3::new(0.0, 0.0, 30.0)], 2.0).unwrap();
    let lip = crate::raise_band(&sh, Vec3::new(0.0, 29.0, 30.0), 0.5, 1.5, false).unwrap();
    check_kept(&lip, &sh);
    let h = lip.history().unwrap();
    let rim = h.faces.iter().find(|f| f.change == Change::Modified).unwrap();
    let [FaceOrigin::Face { operand: 0, face: rim }] = rim.origins.as_slice() else { panic!() };
    // The rim's inner loop: the edges the walls stand on.
    let loop_edges: Vec<usize> =
        crate::history::edge_faces(&sh.solid).iter().enumerate().filter(|(_, fs)| fs.contains(rim)).map(|(i, _)| i).collect();
    let mut walls = 0;
    for (i, f) in h.faces.iter().enumerate() {
        if let [FaceOrigin::Edge { operand: 0, edge }] = f.origins.as_slice() {
            assert_eq!(*edge, nearest_edge(&lip, i, &sh, &loop_edges));
            walls += 1;
        }
    }
    // Two walls per inner edge (the face's and the band's), and the band's top.
    assert_eq!(walls, 8);
    assert_eq!(h.faces.iter().filter(|f| f.origins == [FaceOrigin::Offset { operand: 0, face: *rim }]).count(), 1);
    assert_eq!(h.faces.iter().filter(|f| f.change == Change::Kept).count(), sh.face_count() - 1);
}

#[test]
fn emboss_reports_kept_trimmed_and_profile_faces() {
    let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
    let plane = Plane::new(Vec3::Y * 10.0, -Vec3::X, Vec3::Z).unwrap();
    let profile = Region2 { outer: rotated(&rect(-2.0, 5.0, 2.0, 10.0), 1), holes: vec![] };
    for depth in [1.0, -1.0] {
        let e = crate::emboss_cylinder(&body, &plane, std::slice::from_ref(&profile), depth).unwrap();
        check_kept(&e, &body);
        let h = e.history().unwrap();
        assert_eq!(h.faces.iter().filter(|f| f.change == Change::Kept).count(), body.face_count() - 1);
        assert_eq!(h.faces.iter().filter(|f| f.change == Change::Modified).count(), 1);
        assert_eq!(h.faces.iter().filter(|f| f.origins == [END]).count(), 1);
        // Each wall over its segment: the sketch's y is the cylinder's z, its x the arc length.
        for (pts, f) in points(&e).iter().zip(&h.faces) {
            let [FaceOrigin::Profile { segment }] = f.origins.as_slice() else { continue };
            let back: Vec<Vec2> = pts.iter().map(|p| Vec2::new(10.0 * (-p.x).atan2(p.y), p.z)).collect();
            assert_eq!(under(&[&profile.outer], &back, 1e-6), [*segment]);
        }
    }
}

/// A new edge's meeting: each side all of its face's origins (a merged face's several); none
/// where an origin lies on both sides (pieces of one face) or a face comes from nothing.
#[test]
fn a_new_edge_meets_whole_faces() {
    let f = |face: usize| FaceOrigin::Face { operand: 0, face };
    let blend = FaceOrigin::Edge { operand: 0, edge: 3 };
    let faces = vec![
        FaceProv { origins: vec![f(0), f(1)], change: Change::Merged },
        FaceProv { origins: vec![blend], change: Change::Generated },
        FaceProv { origins: vec![f(1)], change: Change::Split },
        FaceProv { origins: vec![], change: Change::Generated },
    ];
    let merged = Side::new([f(0), f(1)]).unwrap();
    assert_eq!(super::meeting(&[1, 0], &faces), [EdgeOrigin::meet(merged, blend)]);
    assert_eq!(super::meeting(&[2, 1], &faces), [EdgeOrigin::meet(f(1), blend)]);
    assert!(super::meeting(&[0, 2], &faces).is_empty());
    assert!(super::meeting(&[1, 3], &faces).is_empty());
    assert!(super::meeting(&[1], &faces).is_empty());
}
