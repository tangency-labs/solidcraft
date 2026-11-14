//! Guide rails on lofts (#201), checked against analytic solids: a rail bulging out from a loft
//! between two equal circles makes the solid of revolution of the rail, a stretching rail widens
//! squares one way only, two rails carry the sections between them, a sheet loft of lines follows
//! its rail, six rails around a cylinder make their solid of revolution, six rails carry a square
//! into a slot; and rails the loft cannot follow are refused.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use solidcraft_geom::prelude::*;
use std::f64::consts::PI;

use solidcore_record::{Loop2, Plane, Seg2, Vec2, Vec3};

use crate::{KernelError, RailFit, loft, loft_sheet_with_rails, loft_with_rails, measure};

fn at(z: f64) -> Plane {
    Plane { origin: Vec3::new(0.0, 0.0, z), x: Vec3::X, y: Vec3::Y }
}

fn circle(r: f64) -> Loop2 {
    Loop2::circle(Vec2::ZERO, r)
}

fn square(h: f64) -> Loop2 {
    Loop2::polygon(&[Vec2::new(-h, -h), Vec2::new(h, -h), Vec2::new(h, h), Vec2::new(-h, h)])
}

/// The bulge of the rails: 0 at both ends, 1 halfway up the 40 mm loft.
fn bulge(z: f64) -> f64 {
    (PI * z / 40.0).sin()
}

/// A rail at angle `a` about the z axis, `r(z)` out from it, from z = 0 to z = 40.
fn around(a: f64, r: impl Fn(f64) -> f64) -> Vec<Vec3> {
    (0..=80).map(|i| f64::from(i) * 0.5).map(|z| Vec3::new(r(z) * a.cos(), r(z) * a.sin(), z)).collect()
}

/// A rail in the XZ plane at `x(z)`, from z = 0 to z = 40.
fn rail(x: impl Fn(f64) -> f64) -> Vec<Vec3> {
    (0..=80).map(|i| f64::from(i) * 0.5).map(|z| Vec3::new(x(z), 0.0, z)).collect()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs()
}

/// The body's volume from a fine mesh.
fn volume(b: &crate::Body) -> f64 {
    b.volume_at(1e-4).unwrap()
}

/// How far `p` is from the nearest face's surface of the body.
fn off(b: &crate::Body, p: Vec3) -> f64 {
    b.record()
        .faces
        .iter()
        .filter_map(|f| {
            let s = solidcraft_geom::brep::eval::SurfaceEval::new(&f.surface).ok()?;
            let uv = s.project(p, None, f64::INFINITY)?;
            Some(s.point(uv)?.dist(p))
        })
        .fold(f64::INFINITY, f64::min)
}

/// A rail straight up the side of a loft between equal circles changes nothing: the plain loft
/// (the cylinder, to the accuracy of its fitted circles).
#[test]
fn a_straight_rail_up_a_cylinder_leaves_it_a_cylinder() {
    let sections = [(at(0.0), circle(10.0)), (at(40.0), circle(10.0))];
    let b = loft_with_rails(&sections, &[rail(|_| 10.0)], RailFit::Scale).unwrap();
    let m = measure(&b).unwrap();
    let plain = measure(&loft(&sections).unwrap()).unwrap();
    assert!(close(m.volume, 4000.0 * PI, 1e-3), "volume {} vs {}", m.volume, 4000.0 * PI);
    assert!(close(m.volume, plain.volume, 1e-6), "{} vs the plain loft's {}", m.volume, plain.volume);
}

/// A rail bulging out to radius 15 halfway up turns the loft into the rail's solid of
/// revolution: circles of radius r = 10 + 5 sin(πz/40), volume π∫r² = 4500π + 8000. The sections
/// stay exactly as drawn, and three sections give the same solid.
#[test]
fn a_bulging_rail_makes_the_solid_of_revolution_of_the_rail() {
    let want = 4500.0 * PI + 8000.0;
    let r = rail(|z| 10.0 + 5.0 * bulge(z));
    for sections in
        [vec![(at(0.0), circle(10.0)), (at(40.0), circle(10.0))], vec![(at(0.0), circle(10.0)), (at(20.0), circle(15.0)), (at(40.0), circle(10.0))]]
    {
        let b = loft_with_rails(&sections, std::slice::from_ref(&r), RailFit::Scale).unwrap();
        let m = measure(&b).unwrap();
        assert!(close(volume(&b), want, 1e-4), "{} sections: volume {} vs {want}", sections.len(), volume(&b));
        // The loft runs through the rail.
        let worst = r.iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
        assert!(worst < 1e-3, "{} sections: the rail is {worst} mm off the loft", sections.len());
        assert!((m.bbox.max.x - 15.0).abs() < 1e-2 && (m.bbox.max.y - 15.0).abs() < 1e-2, "{:?}", m.bbox);
        assert!(m.bbox.min.z.abs() < 1e-9 && (m.bbox.max.z - 40.0).abs() < 1e-9, "{:?}", m.bbox);
    }
    // The sections given the other way round: the same solid.
    let flipped = [(at(40.0), circle(10.0)), (at(0.0), circle(10.0))];
    let v = volume(&loft_with_rails(&flipped, &[r], RailFit::Scale).unwrap());
    assert!(close(v, want, 1e-4), "reversed: volume {v} vs {want}");
}

/// A rail from the middle of a square's side bulging out by 5: stretching widens the squares
/// only across to the rail (sections 2w × 10 with w = 5 + 5 sin, volume 4000 + 8000/π); scaling
/// grows them both ways (sections (2w)², volume 6000 + 16000/π).
#[test]
fn a_stretching_rail_widens_squares_one_way_and_a_scaling_one_both() {
    let sections = [(at(0.0), square(5.0)), (at(40.0), square(5.0))];
    let r = rail(|z| 5.0 + 5.0 * bulge(z));
    let b = loft_with_rails(&sections, std::slice::from_ref(&r), RailFit::Stretch).unwrap();
    let stretched = measure(&b).unwrap();
    let want = 4000.0 + 8000.0 / PI;
    assert!(close(volume(&b), want, 1e-4), "stretched: volume {} vs {want}", volume(&b));
    assert!((stretched.bbox.max.x - 10.0).abs() < 1e-2 && (stretched.bbox.max.y - 5.0).abs() < 1e-6, "{:?}", stretched.bbox);
    let b = loft_with_rails(&sections, &[r], RailFit::Scale).unwrap();
    let scaled = measure(&b).unwrap();
    let want = 6000.0 + 16000.0 / PI;
    assert!(close(volume(&b), want, 1e-4), "scaled: volume {} vs {want}", volume(&b));
    assert!((scaled.bbox.max.y - 10.0).abs() < 1e-2, "{:?}", scaled.bbox);
}

/// Two rails, one straight up a square's side and one bulging out from the opposite side: the
/// sections run between them, 10 + 5 sin wide. Stretching keeps their depth (volume
/// 4000 + 4000/π); scaling sizes it too (sections (10 + 5 sin)², volume 4500 + 8000/π).
#[test]
fn two_rails_carry_the_sections_between_them() {
    let sections = [(at(0.0), square(5.0)), (at(40.0), square(5.0))];
    let rails = [rail(|_| -5.0), rail(|z| 5.0 + 5.0 * bulge(z))];
    let b = loft_with_rails(&sections, &rails, RailFit::Stretch).unwrap();
    let stretched = measure(&b).unwrap();
    let want = 4000.0 + 4000.0 / PI;
    assert!(close(volume(&b), want, 1e-4), "stretched: volume {} vs {want}", volume(&b));
    for r in &rails {
        let worst = r.iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
        assert!(worst < 1e-3, "a rail is {worst} mm off the loft");
    }
    assert!((stretched.bbox.min.x + 5.0).abs() < 1e-6 && (stretched.bbox.max.x - 10.0).abs() < 1e-2, "{:?}", stretched.bbox);
    let v = volume(&loft_with_rails(&sections, &rails, RailFit::Scale).unwrap());
    let want = 4500.0 + 8000.0 / PI;
    assert!(close(v, want, 1e-4), "scaled: volume {v} vs {want}");
}

/// A sheet lofted between two 20 mm lines with a rail from one end bulging out by 5: the lines
/// between grow about their middles, 2(10 + 5 sin) long; area 800 + 800/π.
#[test]
fn a_sheet_loft_of_lines_follows_its_rail() {
    let line = vec![Seg2::Line { a: Vec2::new(-10.0, 0.0), b: Vec2::new(10.0, 0.0) }];
    let sections = [(at(0.0), line.clone()), (at(40.0), line)];
    let b = loft_sheet_with_rails(&sections, &[rail(|z| 10.0 + 5.0 * bulge(z))], RailFit::Scale).unwrap();
    let m = measure(&b).unwrap();
    let want = 800.0 + 800.0 / PI;
    let worst = rail(|z| 10.0 + 5.0 * bulge(z)).iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
    assert!(worst < 1e-3, "the rail is {worst} mm off the sheet");
    assert!(close(m.area, want, 1e-3), "area {} vs {want}", m.area);
    assert!((m.bbox.max.x - 15.0).abs() < 1e-2 && (m.bbox.min.x + 15.0).abs() < 1e-2, "{:?}", m.bbox);
}

/// Rails the loft cannot follow are refused, saying why: one that starts off the first section,
/// one that stops short of the last, two of three rails on one point, seventeen rails, a third
/// rail off a section, and one up the middle of a sheet loft's lines.
#[test]
fn rails_the_loft_cannot_follow_are_refused() {
    let sections = [(at(0.0), circle(10.0)), (at(40.0), circle(10.0))];
    let refused = |rails: &[Vec<Vec3>]| match loft_with_rails(&sections, rails, RailFit::Scale) {
        Err(KernelError::Invalid(m)) => m,
        other => panic!("{other:?}"),
    };
    assert!(refused(&[rail(|_| 12.0)]).contains("section 1"), "{}", refused(&[rail(|_| 12.0)]));
    let short: Vec<Vec3> = rail(|_| 10.0).into_iter().filter(|p| p.z <= 30.0).collect();
    let m = refused(&[short]);
    assert!(m.contains("does not reach section 2"), "{m}");
    let m = refused(&[rail(|_| 10.0), rail(|_| -10.0), rail(|_| 10.0)]);
    assert!(m.contains("guide rails 1 and 3 meet section 1 at the same point"), "{m}");
    let many: Vec<Vec<Vec3>> = (0..17).map(|k| around(f64::from(k) * 2.0 * PI / 17.0, |_| 10.0)).collect();
    let m = refused(&many);
    assert!(m.contains("1…16"), "{m}");
    // With three or more, an error names the rail, and where it misses.
    let m = refused(&[around(0.0, |_| 10.0), around(2.0, |_| 10.0), around(4.0, |_| 12.0)]);
    assert!(m.contains("guide rail 3") && m.contains("2.000 mm") && m.contains(" at ["), "{m}");
    let line = vec![Seg2::Line { a: Vec2::new(-10.0, 0.0), b: Vec2::new(10.0, 0.0) }];
    match loft_sheet_with_rails(&[(at(0.0), line.clone()), (at(40.0), line)], &[rail(|_| 0.0)], RailFit::Scale) {
        Err(KernelError::Invalid(m)) => assert!(m.contains("middle"), "{m}"),
        other => panic!("{other:?}"),
    }
}

/// Six rails around a loft between equal circles, all bulging out to radius 15 halfway up: the
/// rails' solid of revolution (as one rail makes it), and every rail on the loft.
#[test]
fn six_rails_around_a_cylinder_make_their_solid_of_revolution() {
    let want = 4500.0 * PI + 8000.0;
    let sections = [(at(0.0), circle(10.0)), (at(40.0), circle(10.0))];
    let rails: Vec<Vec<Vec3>> = (0..6).map(|k| around(f64::from(k) * PI / 3.0, |z| 10.0 + 5.0 * bulge(z))).collect();
    let b = loft_with_rails(&sections, &rails, RailFit::Scale).unwrap();
    assert!(close(volume(&b), want, 1e-3), "volume {} vs {want}", volume(&b));
    for (k, r) in rails.iter().enumerate() {
        let worst = r.iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
        assert!(worst < 1e-3, "rail {} is {worst} mm off the loft", k + 1);
    }
}

/// A 40 mm square into an 18 × 6 mm slot, guided by six rails (from its corners and the middles
/// of two sides, each bowing out 3 mm on the way): the loft is built within 2 s and passes within
/// 0.01 mm of every rail.
#[test]
fn six_rails_carry_a_square_into_a_slot() {
    let slot = Loop2::polygon(&[Vec2::new(-9.0, -3.0), Vec2::new(9.0, -3.0), Vec2::new(9.0, 3.0), Vec2::new(-9.0, 3.0)]);
    let sections = [(at(0.0), square(20.0)), (at(40.0), slot)];
    let ends = [
        ((20.0, 20.0), (9.0, 3.0)),
        ((-20.0, 20.0), (-9.0, 3.0)),
        ((-20.0, -20.0), (-9.0, -3.0)),
        ((20.0, -20.0), (9.0, -3.0)),
        ((0.0, 20.0), (0.0, 3.0)),
        ((0.0, -20.0), (0.0, -3.0)),
    ];
    let rails: Vec<Vec<Vec3>> = ends
        .iter()
        .map(|&((x0, y0), (x1, y1))| {
            (0..=80)
                .map(|i| {
                    let z = f64::from(i) * 0.5;
                    let f = z / 40.0;
                    let (x, y) = (x0 + (x1 - x0) * f, y0 + (y1 - y0) * f);
                    let out = Vec2::new(x, y).normalized().unwrap_or(Vec2::ZERO) * (3.0 * bulge(z));
                    Vec3::new(x + out.x, y + out.y, z)
                })
                .collect()
        })
        .collect();
    let t = std::time::Instant::now();
    let b = loft_with_rails(&sections, &rails, RailFit::Scale).unwrap();
    let took = t.elapsed().as_secs_f64();
    assert!(took < 2.0, "the loft took {took:.2} s");
    let m = measure(&b).unwrap();
    assert!(m.volume > 0.0 && m.bbox.min.z.abs() < 1e-9 && (m.bbox.max.z - 40.0).abs() < 1e-9, "{m:?}");
    for (k, r) in rails.iter().enumerate() {
        let worst = r.iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
        assert!(worst < 0.01, "rail {} is {worst} mm off the loft", k + 1);
    }
}

/// A pie of `sweep` radians and radius 10 about the origin: the arc, then two lines through the
/// centre.
fn pie(sweep: f64) -> Loop2 {
    let end = Vec2::new(10.0 * sweep.cos(), 10.0 * sweep.sin());
    Loop2 {
        segs: vec![
            Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: 0.0, sweep },
            Seg2::Line { a: end, b: Vec2::ZERO },
            Seg2::Line { a: Vec2::ZERO, b: Vec2::new(10.0, 0.0) },
        ],
    }
}

/// A rail straight up from `p` (x, y) from z = 0 to z = `top`.
fn straight(p: Vec2, top: f64) -> Vec<Vec3> {
    (0..=80).map(|i| f64::from(i) * top / 80.0).map(|z| Vec3::new(p.x, p.y, z)).collect()
}

/// Sections whose curves differ in kind take rails (#12512): a half-disc (an arc and a line)
/// lofted to itself along a straight rail up the arc is the plain loft, the half-cylinder, exactly
/// (its round wall is the circle at every height); and the half-disc lofts to a rectangle along a
/// bulging rail, which it follows (the rail's point on the sections drifts from the arc's top
/// to the middle of the rectangle's edge, so the scaled stations reach past the rail).
#[test]
fn sections_of_an_arc_and_a_line_take_rails() {
    let half = Loop2 {
        segs: vec![
            Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: 0.0, sweep: PI },
            Seg2::Line { a: Vec2::new(-10.0, 0.0), b: Vec2::new(10.0, 0.0) },
        ],
    };
    let sections = [(at(0.0), half.clone()), (at(40.0), half.clone())];
    let b = loft_with_rails(&sections, &[straight(Vec2::new(0.0, 10.0), 40.0)], RailFit::Scale).unwrap();
    let plain = volume(&loft(&sections).unwrap());
    assert!(close(volume(&b), 2000.0 * PI, 1e-3), "volume {} vs {}", volume(&b), 2000.0 * PI);
    assert!(close(volume(&b), plain, 1e-6), "{} vs the plain loft's {}", volume(&b), plain);
    for z in [5.0, 13.0, 20.0, 31.0] {
        for a in [0.3, 1.1, PI / 2.0, 2.4, 2.9] {
            let p = Vec3::new(10.0 * a.cos(), 10.0 * a.sin(), z);
            assert!(off(&b, p) < 1e-6, "{p:?} is {} mm off the half-cylinder", off(&b, p));
        }
    }
    // The reporter's case: the half-disc to a rectangle, with a rail bulging out from the arc.
    let rect = Loop2::polygon(&[Vec2::new(-20.0, -10.0), Vec2::new(20.0, -10.0), Vec2::new(20.0, 10.0), Vec2::new(-20.0, 10.0)]);
    let sections = [(at(0.0), half), (at(40.0), rect)];
    let r: Vec<Vec3> = (0..=80).map(|i| f64::from(i) * 0.5).map(|z| Vec3::new(0.0, 10.0 + 5.0 * bulge(z), z)).collect();
    let b = loft_with_rails(&sections, std::slice::from_ref(&r), RailFit::Scale).unwrap();
    let m = measure(&b).unwrap();
    let worst = r.iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
    assert!(worst < 1e-3, "the rail is {worst} mm off the loft");
    assert!(m.volume > 40.0 * 50.0 * PI && m.volume < 40.0 * 800.0, "volume {}", m.volume);
    assert!(m.bbox.max.y > 15.0 - 1e-2 && m.bbox.min.z.abs() < 1e-9 && (m.bbox.max.z - 40.0).abs() < 1e-9, "{:?}", m.bbox);
}

/// Arcs of different turns (one piece against two, different weights) pair up too: a quarter pie
/// to a third-of-a-turn pie along a straight rail up their shared centre corner is the plain
/// loft to a tenth of a percent (the stations between two arcs of different weights are not
/// the ruled surface's sections exactly).
#[test]
fn arcs_of_different_turns_take_rails() {
    let sections = [(at(0.0), pie(PI / 2.0)), (at(40.0), pie(2.0 * PI / 3.0))];
    let r = straight(Vec2::ZERO, 40.0);
    let b = loft_with_rails(&sections, std::slice::from_ref(&r), RailFit::Scale).unwrap();
    let plain = volume(&loft(&sections).unwrap());
    assert!(close(volume(&b), plain, 1e-3), "{} vs the plain loft's {}", volume(&b), plain);
    assert!(volume(&b) > 40.0 * 25.0 * PI && volume(&b) < 40.0 * 100.0 * PI / 3.0, "volume {}", volume(&b));
    let worst = r.iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
    assert!(worst < 1e-6, "the rail is {worst} mm off the loft");
}

/// A section on the plane through the origin with axes `x` and `y` (normal x × y), with no curves:
/// enough for the normals.
fn facing(x: Vec3, y: Vec3) -> crate::loft_rails::Section {
    crate::loft_rails::Section { plane: Plane { origin: Vec3::ZERO, x, y }, curves: Vec::new(), numbers: Vec::new() }
}

/// The sections' oriented normals, with the loft's direction `d(v)` at each section.
fn normals(sections: &[crate::loft_rails::Section], d: impl Fn(f64) -> Option<Vec3>) -> Vec<Vec3> {
    let along: Vec<f64> = (0..sections.len()).map(|i| i as f64 / (sections.len() - 1) as f64).collect();
    crate::loft_rails::section_normals(sections, d, &along)
}

fn same(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-12
}

/// Section normals for the stations (#12031). A keel on a vertical plane and a sheer on a level
/// one, at 90°, face along the loft whichever way the keel's sketch faces. Sections within 60°
/// of each other turn the short way, even where the loft's direction reverses. A first section
/// that the loft can't decide, because its plane lies along the loft or the loft has no
/// direction there, takes its facing from the next section that the loft decides.
#[test]
fn section_normals_face_along_the_loft_and_turn_the_short_way() {
    let (x, y, z) = (Vec3::X, Vec3::Y, Vec3::new(0.0, 0.0, 1.0));
    let up_and_over = |_: f64| Some(Vec3::new(0.1, 0.6, 0.8));
    let level = facing(x, y);
    for keel in [facing(x, z), facing(z, x)] {
        let n = normals(&[keel, level.clone()], up_and_over);
        assert!(same(n[0], y) && same(n[1], z), "{n:?}");
    }
    // Three level sections, the loft rising to the second and falling to the third.
    let n = normals(&[level.clone(), facing(y, x), level.clone()], |v| Some(if v < 0.4 { z } else { z * -1.0 }));
    assert!(n.iter().all(|m| same(*m, n[0])), "{n:?}");
    // A first section along the loft (straight up), then one tilted 20° from level.
    let (c, s) = (20f64.to_radians().cos(), 20f64.to_radians().sin());
    let tilted = facing(x, Vec3::new(0.0, c, s));
    for first in [facing(x, z), facing(z, x)] {
        let n = normals(&[first, tilted.clone()], |_| Some(z));
        assert!(n[0].dot(n[1]) > 0.0 && n[1].dot(z) > 0.0, "{n:?}");
    }
    // No loft direction at the first section.
    for first in [facing(x, z), facing(z, x)] {
        let n = normals(&[first, tilted.clone()], |v| (v > 0.0).then_some(Vec3::new(0.0, 0.6, 0.8)));
        assert!(n[0].dot(n[1]) > 0.0 && n[1].dot(z) > 0.0, "{n:?}");
    }
}
