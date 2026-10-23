//! A lip swept along the rim of a round body joins it, flush with its side or set in from it:
//! the lip's foot lies on the body's top, and its sides are round where the rim is. A plug filling
//! a gap between two walls with a round top joins them where its corners lie a hair off their arc.

use std::f64::consts::{FRAC_PI_2, PI};

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::*;

/// A 1 × 1 mm lip on the top (z = 10) of a round body of radius 10 about Z, its outer side
/// `inset` in from the body's side, swept a quarter turn from the XZ plane.
fn lip(inset: f64) -> Body {
    let plane = Plane::new(Vec3::ZERO, Vec3::X, Vec3::Z).unwrap();
    let (x0, x1) = (9.0 - inset, 10.0 - inset);
    let outer = Loop2::polygon(&[Vec2::new(x0, 10.0), Vec2::new(x1, 10.0), Vec2::new(x1, 11.0), Vec2::new(x0, 11.0)]);
    let path = [PathSeg::Arc { a: Vec3::new(10.0, 0.0, 10.0), center: Vec3::new(0.0, 0.0, 10.0), axis: Vec3::Z, angle: FRAC_PI_2 }];
    sweep(&plane, &Region2 { outer, holes: vec![] }, &path).unwrap()
}

#[test]
fn a_lip_swept_on_a_round_rim_joins_it() {
    let body = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 10.0).unwrap();
    for inset in [0.0, 0.5] {
        let (r0, r1) = (9.0 - inset, 10.0 - inset);
        // Measured as the result is (the round faces' volume from their meshes).
        let mesh_volume = |b: &Body| b.tessellate(1e-5).unwrap().measure().volume;
        let want = mesh_volume(&body) + mesh_volume(&lip(inset));
        assert!((want - PI * 1000.0 - PI * (r1 * r1 - r0 * r0) / 4.0).abs() < 0.05);
        for (a, b) in [(&body, &lip(inset)), (&lip(inset), &body)] {
            let (r, d) = capture_boolean_diagnostics(|| boolean(a, b, BoolOp::Union));
            let r = r.unwrap().unwrap();
            assert_eq!(r.shell_count(), 1, "inset {inset}: one solid");
            assert!(r.validity().is_empty(), "inset {inset}: {:?}", r.validity());
            let v = mesh_volume(&r);
            assert!((v - want).abs() < 1e-3, "inset {inset}: volume {v} vs {want}");
            assert!(!d.events.iter().any(|e| e.strategy == BooleanStrategy::ToolNudge), "inset {inset}: an exact join: {d:?}");
        }
    }
}

/// The swept lip's foot over the turn is a plane, so it is found lying on the body's top.
#[test]
fn a_swept_lips_foot_is_a_plane() {
    let l = lip(0.0);
    let foot = crate::coplanar::faces_in(&l, -Vec3::Z, -10.0, 1e-6);
    let m = l.tessellate(1e-5).unwrap();
    let area: f64 = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, f)| foot.contains(&(**f as usize)))
        .filter_map(|(t, _)| m.tri(t))
        .map(|[a, b, c]| (b - a).cross(c - a).len() / 2.0)
        .sum();
    assert!((area - PI * (100.0 - 81.0) / 4.0).abs() < 1e-3, "{} faces, {area} mm²", foot.len());
}

/// An arm of two walls with a round top (r 11.75 about Z, cut flat 6 below its axis), z 0..10 and
/// 16.8..30, held together by a bar beside them; with `holes`, screw holes r 2.075 through the
/// lower wall.
fn walls(holes: bool) -> Body {
    let r = 11.75;
    let xc = (r * r - 36.0f64).sqrt();
    let outer = Loop2 {
        segs: vec![
            Seg2::Line { a: Vec2::new(-xc, -6.0), b: Vec2::new(xc, -6.0) },
            Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: r, start: (-6.0f64).atan2(xc), sweep: PI + 2.0 * (6.0 / r).asin() },
        ],
    };
    let cap = |z: f64, h: f64| {
        extrude(&Plane::new(Vec3::new(0.0, 0.0, z), Vec3::X, Vec3::Y).unwrap(), &[Region2 { outer: outer.clone(), holes: vec![] }], 0.0, h)
            .unwrap()
            .remove(0)
    };
    let mut lower = cap(0.0, 10.0);
    if holes {
        for x in [-5.0, 5.0] {
            lower = boolean(&lower, &cylinder(Vec3::new(x, 0.0, -1.0), Vec3::Z, 2.075, 12.0).unwrap(), BoolOp::Cut).unwrap().unwrap();
        }
    }
    let both = boolean(&lower, &cap(16.8, 13.2), BoolOp::Union).unwrap().unwrap();
    boolean(&both, &box_solid(Vec3::new(11.0, -6.0, 0.0), Vec3::new(16.0, -2.0, 30.0)).unwrap(), BoolOp::Union).unwrap().unwrap()
}

/// The plug filling the gap z 10..16.8 between [`walls`]: their profile within |x| <= 9, its arc
/// about a centre `d` off theirs (a sketch solved on its own, as the walls' was), so its corners on
/// the arc lie a hair off the walls' edge; with `holes`, the screw holes go on through it.
fn plug(d: f64, holes: bool) -> Body {
    let r = 11.75;
    let y = |x: f64| (r * r - (x - d) * (x - d)).sqrt();
    let (s0, s1) = (y(9.0).atan2(9.0 - d), y(-9.0).atan2(-9.0 - d));
    let outer = Loop2 {
        segs: vec![
            Seg2::Line { a: Vec2::new(-9.0, -6.0), b: Vec2::new(9.0, -6.0) },
            Seg2::Line { a: Vec2::new(9.0, -6.0), b: Vec2::new(9.0, y(9.0)) },
            Seg2::Arc { center: Vec2::new(d, 0.0), radius: r, start: s0, sweep: s1 - s0 },
            Seg2::Line { a: Vec2::new(-9.0, y(-9.0)), b: Vec2::new(-9.0, -6.0) },
        ],
    };
    let holes: Vec<Loop2> =
        if holes { [-5.0, 5.0].iter().map(|x| Loop2::circle(Vec2::new(*x, 0.0), 2.075).reversed()).collect() } else { Vec::new() };
    extrude(&Plane::new(Vec3::new(0.0, 0.0, 10.0), Vec3::X, Vec3::Y).unwrap(), &[Region2 { outer, holes }], 0.0, 6.8).unwrap().remove(0)
}

/// The plug only touches the walls, back to back at both its ends. Its corners on the round top
/// lie a few microns off the walls' arc: near enough for the walls' edge to be split there, too far
/// for the edge to take them as its own points, which failed the exact join, and the nudged tries
/// after it, with "no result" (seconds, or minutes on a larger part). Exact now, with the screw
/// holes going on from the wall's too, where the bores' common cylinder used to send the join the
/// long way round (narrowed, joined and cut again).
#[test]
fn a_plug_whose_corners_lie_a_hair_off_the_walls_arc_joins_them() {
    let mesh_volume = |b: &Body| b.tessellate(1e-4).unwrap().measure().volume;
    for holes in [false, true] {
        let w = walls(holes);
        for d in [0.0, 2e-6, 3e-6] {
            let p = plug(d, holes);
            let want = mesh_volume(&w) + mesh_volume(&p);
            let t = std::time::Instant::now();
            let (r, diag) = capture_boolean_diagnostics(|| boolean(&w, &p, BoolOp::Union));
            let r = r.unwrap_or_else(|e| panic!("holes {holes}, off {d}: {e}")).unwrap();
            assert_eq!(r.shell_count(), 1, "holes {holes}, off {d}: one solid");
            assert!(r.validity().is_empty(), "holes {holes}, off {d}: {:?}", r.validity());
            let v = mesh_volume(&r);
            assert!((v - want).abs() < want * 1e-5, "holes {holes}, off {d}: volume {v} vs {want}");
            // Glued along the contact, with no retry (nudged, or narrowed and cut again).
            assert!(diag.events.is_empty(), "holes {holes}, off {d}: glued at once: {diag:?}");
            println!("holes {holes}, off {d}: {} faces in {:?}", r.face_count(), t.elapsed());
        }
    }
}
