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

/// The end on the first section wins even when the other rail end is closer to its centroid.
#[test]
fn a_rail_is_oriented_by_its_first_section_not_its_centroid() {
    let sections = [
        (Plane { origin: Vec3::new(100.0, 0.0, 0.0), x: Vec3::X, y: Vec3::Z }, circle(100.0)),
        (Plane { origin: Vec3::new(100.0, 27.0, 0.0), x: Vec3::X, y: Vec3::Z }, circle(75.0)),
    ];
    let r = [Vec3::ZERO, Vec3::new(25.0, 27.0, 0.0)];
    let b = loft_with_rails(&sections, &[r.to_vec()], RailFit::Scale).unwrap();
    let worst = r.iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
    assert!(worst < 1e-2, "the rail is {worst} mm off the loft");
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

/// Two rails that run 5 mm on past the last of three sections (#17238), bulging out to radius 15
/// halfway up: they meet the last section where they cross its plane, and the loft ends there,
/// the solid of revolution of the rails (volume 4500π + 8000), as rails that end at it make.
#[test]
fn rails_that_run_on_past_the_last_section_meet_it_where_they_cross_it() {
    let want = 4500.0 * PI + 8000.0;
    let sections = [(at(0.0), circle(10.0)), (at(20.0), circle(15.0)), (at(40.0), circle(10.0))];
    let longer =
        |side: f64| -> Vec<Vec3> { (0..=90).map(|i| f64::from(i) * 0.5).map(|z| Vec3::new(side * (10.0 + 5.0 * bulge(z)), 0.0, z)).collect() };
    let b = loft_with_rails(&sections, &[longer(1.0), longer(-1.0)], RailFit::Scale).unwrap();
    let ending = [rail(|z| 10.0 + 5.0 * bulge(z)), rail(|z| -10.0 - 5.0 * bulge(z))];
    let ended = volume(&loft_with_rails(&sections, &ending, RailFit::Scale).unwrap());
    assert!(close(volume(&b), want, 1e-4), "volume {} vs {want}", volume(&b));
    assert!(close(volume(&b), ended, 1e-9), "volume {} vs {ended} with rails that end at the last section", volume(&b));
    let m = measure(&b).unwrap();
    assert!(m.bbox.min.z.abs() < 1e-9 && (m.bbox.max.z - 40.0).abs() < 1e-9, "{:?}", m.bbox);
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

/// A section that is one straight line lies in every plane through it, so the plane of the
/// sketch it was drawn in says nothing about the loft. Here the bow's line is drawn on XZ, the
/// plane the keel rail lies in; the loft still follows both rails, from either end, and stays
/// between its two sections (#4056).
#[test]
fn a_line_section_drawn_in_a_rails_plane_still_takes_the_rail() {
    let stern = (
        Plane { origin: Vec3::new(100.0, 0.0, 0.0), x: Vec3::Y, y: Vec3::Z },
        vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(30.0, 5.0) }, Seg2::Line { a: Vec2::new(30.0, 5.0), b: Vec2::new(40.0, 20.0) }],
    );
    // XZ's y runs down world Z: the bow line rises from the keel to the sheer at x = 0.
    let bow = (Plane::XZ, vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(0.0, -20.0) }]);
    let keel: Vec<Vec3> = (0..=50).map(|i| Vec3::new(f64::from(i) * 2.0, 0.0, 0.0)).collect();
    let sheer: Vec<Vec3> = (0..=50).map(|i| Vec3::new(f64::from(i) * 2.0, (f64::from(i) * PI / 100.0).sin() * 40.0, 20.0)).collect();
    for sections in [[stern.clone(), bow.clone()], [bow.clone(), stern.clone()]] {
        let b = loft_sheet_with_rails(&sections, &[keel.clone(), sheer.clone()], RailFit::Scale).unwrap();
        let worst = keel.iter().chain(&sheer).map(|p| off(&b, *p)).fold(0.0, f64::max);
        assert!(worst < 1e-2, "a rail is {worst} mm off the sheet");
        let m = measure(&b).unwrap();
        assert!(m.bbox.min.x > -1e-3 && m.bbox.max.x < 100.0 + 1e-3, "{:?}", m.bbox);
    }
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
    assert!(m.contains("the guide rails meet a section at the same point"), "{m}");
    let many: Vec<Vec<Vec3>> = (0..17).map(|k| around(f64::from(k) * 2.0 * PI / 17.0, |_| 10.0)).collect();
    let m = refused(&many);
    assert!(m.contains("1…16"), "{m}");
    // With three or more, solidcore words the refusal (the rail is not numbered).
    let m = refused(&[around(0.0, |_| 10.0), around(2.0, |_| 10.0), around(4.0, |_| 12.0)]);
    assert!(m.contains("a guide rail crosses a section's plane away from its curves"), "{m}");
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
/// of two sides, each bowing out 3 mm on the way): the loft passes within 0.01 mm of every rail.
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
    let b = loft_with_rails(&sections, &rails, RailFit::Scale).unwrap();
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
    // PROPOSED, for the maintainer to choose. The rectangle prism (40 x 800) was no derived cap:
    // each station is the plain section scaled about its middle until the guided point (the top
    // segment's middle) reaches the rail, which bulges 5 mm past the rectangle's top edge, so
    // the stations swell past the prism. Derived cap: the box around every face's and every
    // edge's control points bounds the solid (`control_box`), so its volume bounds the loft's.
    let (lo, hi) = control_box(&b);
    let d = hi - lo;
    let cap = d.x * d.y * d.z;
    assert!(m.volume > 40.0 * 50.0 * PI && m.volume < cap, "volume {} (control box {cap})", m.volume);
    // Reference, MEASURED, not a bound: a continuum-station model of this construction (the
    // exact affine station map, its volume by the divergence theorem, Richardson-extrapolated)
    // gives 33500.62 mm3; the kernel's 17 wires give 33502.97, 7.0e-5 relative. Within 2e-4
    // (about 6.7 mm3, three times that gap); main's partly twisted pairing gave 31319.95, 6.5% off.
    assert!(close(m.volume, 33500.62, 2e-4), "volume {} vs the continuum model's 33500.62", m.volume);
    assert!(m.bbox.max.y > 15.0 - 1e-2 && m.bbox.min.z.abs() < 1e-9 && (m.bbox.max.z - 40.0).abs() < 1e-9, "{:?}", m.bbox);
}

/// Sections whose curves differ in kind take three or more rails (#12512): the half-disc of the
/// test above to its rectangle, guided by four straight rails (the arc's top, its two shoulders
/// and the line's middle, each to the rectangle's edge). Every rail is within 0.01 mm of the loft.
#[test]
fn sections_of_an_arc_and_a_line_take_four_rails() {
    let half = Loop2 {
        segs: vec![
            Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: 0.0, sweep: PI },
            Seg2::Line { a: Vec2::new(-10.0, 0.0), b: Vec2::new(10.0, 0.0) },
        ],
    };
    let rect = Loop2::polygon(&[Vec2::new(-20.0, -10.0), Vec2::new(20.0, -10.0), Vec2::new(20.0, 10.0), Vec2::new(-20.0, 10.0)]);
    let sections = [(at(0.0), half), (at(40.0), rect)];
    let c = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
    let ends = [((0.0, 10.0), (0.0, 10.0)), ((c, c), (c, 10.0)), ((-c, c), (-c, 10.0)), ((5.0, 0.0), (5.0, -10.0))];
    let rails: Vec<Vec<Vec3>> = ends
        .iter()
        .map(|&((x0, y0), (x1, y1))| {
            (0..=80)
                .map(|i| {
                    let f = f64::from(i) / 80.0;
                    Vec3::new(x0 + (x1 - x0) * f, y0 + (y1 - y0) * f, 40.0 * f)
                })
                .collect()
        })
        .collect();
    let b = loft_with_rails(&sections, &rails, RailFit::Scale).unwrap();
    let m = measure(&b).unwrap();
    let worst = rails.iter().flatten().map(|p| off(&b, *p)).fold(0.0, f64::max);
    assert!(worst < 1e-2, "a rail is {worst} mm off the loft");
    assert!(m.volume > 40.0 * 50.0 * PI && m.volume < 40.0 * 800.0, "volume {}", m.volume);
    assert!(m.bbox.min.z.abs() < 1e-9 && (m.bbox.max.z - 40.0).abs() < 1e-9, "{:?}", m.bbox);
}

/// A cubic section and an arc section, guided by three rails (from the cubic's ends and its top to
/// the arc's ends and top): the cubic is not a line, so it may not take the arc's weights. Either
/// the loft is refused as curves that do not pair, or it passes through the drawn cubic.
#[test]
fn a_cubic_section_and_an_arc_section_take_three_rails() {
    let bow = Loop2 {
        segs: vec![
            cubics(&[[[-10.0, 0.0], [-5.0, 12.0], [5.0, 12.0], [10.0, 0.0]]]).remove(0),
            Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(-10.0, 0.0) },
        ],
    };
    let half = Loop2 {
        segs: vec![
            Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: 0.0, sweep: PI },
            Seg2::Line { a: Vec2::new(-10.0, 0.0), b: Vec2::new(10.0, 0.0) },
        ],
    };
    let sections = [(at(0.0), bow), (at(40.0), half)];
    let ends = [((-10.0, 0.0), (-10.0, 0.0)), ((10.0, 0.0), (10.0, 0.0)), ((0.0, 9.0), (0.0, 10.0))];
    let rails: Vec<Vec<Vec3>> = ends
        .iter()
        .map(|&((x0, y0), (x1, y1))| {
            (0..=80)
                .map(|i| {
                    let f = f64::from(i) / 80.0;
                    Vec3::new(x0 + (x1 - x0) * f, y0 + (y1 - y0) * f, 40.0 * f)
                })
                .collect()
        })
        .collect();
    match loft_with_rails(&sections, &rails, RailFit::Scale) {
        Err(KernelError::Invalid(m)) => assert!(m.contains("do not pair"), "{m}"),
        Err(e) => panic!("{e:?}"),
        Ok(b) => {
            for i in 0..=50 {
                let t = f64::from(i) / 50.0;
                let s = 1.0 - t;
                let x = s * s * s * -10.0 + 3.0 * s * s * t * -5.0 + 3.0 * s * t * t * 5.0 + t * t * t * 10.0;
                let y = 3.0 * s * s * t * 12.0 + 3.0 * s * t * t * 12.0;
                // The cap at z = 0 is planar and holds the cubic whatever the loft's curved faces
                // do there, so only the curved faces count.
                let worst = b
                    .record()
                    .faces
                    .iter()
                    .filter(|f| {
                        !matches!(f.surface, solidcore_record::brep::Surface::Plane { .. } | solidcore_record::brep::Surface::PlaneThrough { .. })
                    })
                    .filter_map(|f| {
                        let s = solidcraft_geom::brep::eval::SurfaceEval::new(&f.surface).ok()?;
                        let uv = s.project(Vec3::new(x, y, 0.0), None, f64::INFINITY)?;
                        Some(s.point(uv)?.dist(Vec3::new(x, y, 0.0)))
                    })
                    .fold(f64::INFINITY, f64::min);
                assert!(worst < 2e-5, "the drawn cubic at t = {t} is {worst} mm off the loft's curved faces");
            }
        }
    }
}

/// A square lofted to the same square with a vertex in the middle of its top side, along a
/// straight rail up that vertex (scaled to fit): the vertex splits the first square's top side
/// where it lines up, so every station is the square and the loft is the 20 x 20 x 40 box. (When
/// the square's first side was halved instead, the bottom's middle joined the top's and the walls
/// twisted.)
#[test]
fn a_vertex_only_one_section_has_takes_a_straight_rail() {
    let square = Loop2::polygon(&[Vec2::new(-10.0, -10.0), Vec2::new(10.0, -10.0), Vec2::new(10.0, 10.0), Vec2::new(-10.0, 10.0)]);
    let marked =
        Loop2::polygon(&[Vec2::new(-10.0, -10.0), Vec2::new(10.0, -10.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0), Vec2::new(-10.0, 10.0)]);
    let r = straight(Vec2::new(0.0, 10.0), 40.0);
    let b = loft_with_rails(&[(at(0.0), square), (at(40.0), marked)], std::slice::from_ref(&r), RailFit::Scale).unwrap();
    assert!(close(volume(&b), 16_000.0, 1e-9), "volume {}", volume(&b));
    let worst = r.iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
    assert!(worst < 1e-6, "the rail is {worst} mm off the loft");
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
    crate::loft_rails::Section { plane: Plane { origin: Vec3::ZERO, x, y }, curves: Vec::new(), numbers: Vec::new(), segs: Vec::new() }
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

// The hull side from #16754 as sketch splines draw it: the natural cubic through the issue's
// points with chord-length parameters, one cubic Bézier (its control points) per span. The keel's
// are in its sketch's (x, z), the sheer's in its sketch's (x, y), the rails' in world (x, y, z).
const KEEL: [[[f64; 2]; 4]; 2] = [
    [[0.0, -160.0], [1627.7635723078342, -388.7205147027603], [3255.527144615669, -617.4410294055205], [4880.0, -600.0]],
    [[4880.0, -600.0], [6589.094991330999, -581.6504314603769], [8294.5474956655, -290.82521573018846], [10000.0, 0.0]],
];
const SHEER: [[[f64; 2]; 4]; 2] = [
    [[0.0, 40.0], [1645.285360344809, 727.5583753839488], [3290.5707206896172, 1415.1167507678974], [5000.0, 1650.0]],
    [[5000.0, 1650.0], [6627.886455125052, 1873.6788994950143], [8313.943227562526, 1686.8394497475072], [10000.0, 1500.0]],
];
const STEM: [[[f64; 3]; 4]; 3] = [
    [
        [0.0, 40.0, 1200.0],
        [0.0, 44.00238687713236, 828.3597964181283],
        [0.0, 48.00477375426472, 456.7195928362564],
        [0.0, 45.373332545769244, -62.868622627622926],
    ],
    [
        [0.0, 45.373332545769244, -62.868622627622926],
        [0.0, 45.17215275750797, -102.59234772267868],
        [0.0, 44.93219852666387, -143.18082264512174],
        [0.0, 17.293450399707694, -155.24526595568008],
    ],
    [
        [0.0, 17.293450399707694, -155.24526595568008],
        [0.0, 12.159265663775223, -157.48636169590935],
        [0.0, 6.079632831887611, -158.74318084795468],
        [0.0, 0.0, -160.0],
    ],
];
const TRANSOM: [[[f64; 3]; 4]; 2] = [
    [
        [10000.0, 0.0, 0.0],
        [10000.0, 582.4951876137216, 23.059939931657393],
        [10000.0, 1164.990375227443, 46.11987986331485],
        [10000.0, 1377.2647194380875, 394.8316231623623],
    ],
    [
        [10000.0, 1377.2647194380875, 394.8316231623623],
        [10000.0, 1497.936056877575, 593.0633623827879],
        [10000.0, 1498.9680284387875, 896.5316811913939],
        [10000.0, 1500.0, 1200.0],
    ],
];

/// Cubic Béziers as a section's curves.
fn cubics(pieces: &[[[f64; 2]; 4]]) -> Vec<Seg2> {
    let v = |p: [f64; 2]| Vec2::new(p[0], p[1]);
    pieces.iter().map(|[a, b, c, d]| Seg2::Cubic { p0: v(*a), p1: v(*b), p2: v(*c), p3: v(*d) }).collect()
}

/// Cubic Béziers joined end to end as a rail: the B-spline they make, as solidcore's tessellation
/// samples a curve, every chord within a thousandth of a millimetre of it.
fn cubic_rail(pieces: &[[[f64; 3]; 4]]) -> Vec<Vec3> {
    let mut points: Vec<[f64; 3]> = pieces.first().map(|p| vec![p[0]]).unwrap_or_default();
    let mut knots = vec![0.0; 4];
    for (i, p) in pieces.iter().enumerate() {
        points.extend_from_slice(&p[1..]);
        knots.extend([(i + 1) as f64; 3]);
    }
    knots.push(pieces.len() as f64);
    let curve = solidcore_record::brep::Curve::Spline { knots, points };
    let tolerance = solidcore_tess::MeshTolerance::chord(1e-3);
    solidcore_tess::curve_points(&curve, [0.0, pieces.len() as f64], tolerance).unwrap().into_iter().map(|[x, y, z]| Vec3::new(x, y, z)).collect()
}

/// A 10 m hull side (#16754): a keel and a sheer as sections, the stem and the transom as rails.
/// The stem runs straight down from the sheer and then turns about 90° within some 120 mm into
/// the forefoot. The loft's edges follow both rails all along, not only at its stations: every
/// point of each rail is within 0.5 mm of the sheet, half the loft's tolerance (1e-4 of its size).
/// With only even stations the stem strayed 2.7 mm from the sheet in the forefoot.
#[test]
fn a_hull_side_follows_a_stem_that_turns_tightly() {
    let keel_plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::new(0.0, 0.0, 1.0) };
    let sections = [(keel_plane, cubics(&KEEL)), (at(1200.0), cubics(&SHEER))];
    let rails = [cubic_rail(&STEM), cubic_rail(&TRANSOM)];
    for fit in [RailFit::Scale, RailFit::Stretch] {
        let b = loft_sheet_with_rails(&sections, &rails, fit).unwrap();
        for (name, r) in ["stem", "transom"].iter().zip(&rails) {
            let (worst, p) = r.iter().map(|p| (off(&b, *p), *p)).fold((0.0, Vec3::ZERO), |a, b| if b.0 > a.0 { b } else { a });
            assert!(worst < 0.5, "{fit:?}: the {name} is {worst:.3} mm off the loft at {p:?}");
        }
    }
}

/// The box around the control points of every face's surface and every edge's curve of `b`'s
/// record (each face's NURBS form where it has one). A B-spline or NURBS surface with positive
/// weights lies in the convex hull of its control points, and a trimmed face in its surface, so
/// in that box; so does every B-spline or NURBS edge curve and every line between its ends. A
/// planar face lies in the convex hull of its boundary, its edges, so in the box too. Hence the
/// box holds the whole solid. Any other surface or curve kind panics: the argument does not
/// cover it.
fn control_box(b: &crate::Body) -> (Vec3, Vec3) {
    use solidcore_record::brep as rb;
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    let mut add = |p: [f64; 3]| {
        lo = Vec3::new(lo.x.min(p[0]), lo.y.min(p[1]), lo.z.min(p[2]));
        hi = Vec3::new(hi.x.max(p[0]), hi.y.max(p[1]), hi.z.max(p[2]));
    };
    let unweight = |h: [f64; 4]| {
        assert!(h[3] > 0.0, "a weight {} is not positive", h[3]);
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    };
    for f in &b.record.faces {
        match f.nurbs.as_ref().unwrap_or(&f.surface) {
            rb::Surface::Spline { points, .. } => points.iter().flatten().for_each(|p| add(*p)),
            rb::Surface::RationalSpline { homogeneous, .. } => homogeneous.iter().flatten().for_each(|h| add(unweight(*h))),
            rb::Surface::Plane { .. } | rb::Surface::PlaneThrough { .. } => {}
            other => panic!("a face's surface the control box does not cover: {other:?}"),
        }
    }
    for e in &b.record.edges {
        match e.nurbs.as_ref().unwrap_or(&e.curve) {
            rb::Curve::Line { start, end } => {
                add(*start);
                add(*end);
            }
            rb::Curve::Polyline { points } | rb::Curve::Spline { points, .. } => points.iter().for_each(|p| add(*p)),
            rb::Curve::RationalSpline { homogeneous, .. } => homogeneous.iter().for_each(|h| add(unweight(*h))),
            other => panic!("an edge's curve the control box does not cover: {other:?}"),
        }
    }
    (lo, hi)
}

/// A rail to the apex of a loft to a point (#20985), from the section at z = 0 to (0, 0, 30), at
/// `x(z)` in the XZ plane.
fn rail_to_apex(x: impl Fn(f64) -> f64) -> Vec<Vec3> {
    (0..=60).map(|i| f64::from(i) * 0.5).map(|z| Vec3::new(if z >= 30.0 { 0.0 } else { x(z) }, 0.0, z)).collect()
}

/// A circle of radius 20 lofted to a point 30 mm above its centre along a straight rail up the
/// cone's side changes nothing: the cone, π r² h / 3 = 4000π, the base as drawn.
#[test]
fn a_straight_rail_to_the_apex_leaves_the_cone() {
    let apex = Vec3::new(0.0, 0.0, 30.0);
    let r = rail_to_apex(|z| 20.0 * (1.0 - z / 30.0));
    let b = crate::loft_to_point_with_rails(&at(0.0), &circle(20.0), apex, std::slice::from_ref(&r), RailFit::Scale).unwrap();
    let want = 4000.0 * PI;
    assert!(close(volume(&b), want, 1e-4), "volume {} vs {want}", volume(&b));
    let m = measure(&b).unwrap();
    assert!(m.bbox.min.z.abs() < 1e-9 && (m.bbox.max.z - 30.0).abs() < 1e-6, "{:?}", m.bbox);
    let worst = r.iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
    assert!(worst < 1e-3, "the rail is {worst} mm off the loft");
}

/// A rail curving in to the apex, r = 20 (1 − (z/30)²), makes the rail's solid of revolution:
/// π∫₀³⁰ r² dz = 12000π ∫₀¹ (1 − t²)² dt = 6400π.
#[test]
fn a_curved_rail_to_the_apex_makes_the_solid_of_revolution_of_the_rail() {
    let apex = Vec3::new(0.0, 0.0, 30.0);
    let r = rail_to_apex(|z| 20.0 * (1.0 - (z / 30.0).powi(2)));
    let b = crate::loft_to_point_with_rails(&at(0.0), &circle(20.0), apex, std::slice::from_ref(&r), RailFit::Scale).unwrap();
    let want = 6400.0 * PI;
    assert!(close(volume(&b), want, 1e-3), "volume {} vs {want}", volume(&b));
    let worst = r.iter().map(|p| off(&b, *p)).fold(0.0, f64::max);
    assert!(worst < 1e-2, "the rail is {worst} mm off the loft");
}

/// Two straight rails up opposite corners of a square of half side 10 to the point 30 mm up: the
/// pyramid, 20² · 30 / 3 = 4000. The rails meet at the apex, which is where they have to.
#[test]
fn two_rails_to_the_apex_of_a_pyramid() {
    let apex = Vec3::new(0.0, 0.0, 30.0);
    let edge = |s: f64| -> Vec<Vec3> {
        (0..=60).map(|i| f64::from(i) / 60.0).map(|t| Vec3::new(s * 10.0 * (1.0 - t), s * 10.0 * (1.0 - t), 30.0 * t)).collect()
    };
    let b = crate::loft_to_point_with_rails(&at(0.0), &square(10.0), apex, &[edge(1.0), edge(-1.0)], RailFit::Scale).unwrap();
    assert!(close(volume(&b), 4000.0, 1e-4), "volume {}", volume(&b));
}

/// A rail that ends beside the apex does not reach the point the loft goes to: refused.
#[test]
fn a_rail_that_misses_the_apex_is_refused() {
    let apex = Vec3::new(0.0, 0.0, 30.0);
    let r: Vec<Vec3> = (0..=60).map(|i| f64::from(i) * 0.5).map(|z| Vec3::new(20.0 - z * 0.5, 0.0, z)).collect();
    let e = crate::loft_to_point_with_rails(&at(0.0), &circle(20.0), apex, &[r], RailFit::Scale).unwrap_err();
    assert!(matches!(e, KernelError::Invalid(_)), "{e:?}");
}

/// A square of half side 10 lofted to a point 30 up along a rail from the middle of one side
/// curving in as x = 10 (1 − t²), t = z / 30. Stretching widens each section only across to the
/// rail: 2 · 10 (1 − t²) by 2 · 10 (1 − t), volume 12000 ∫₀¹ (1 − t²)(1 − t) dt = 5000. Scaling
/// grows it both ways: (2 · 10 (1 − t²))², volume 12000 ∫₀¹ (1 − t²)² dt = 6400.
#[test]
fn a_stretching_rail_to_the_apex_widens_one_way_and_a_scaling_one_both() {
    let apex = Vec3::new(0.0, 0.0, 30.0);
    let r = rail_to_apex(|z| 10.0 * (1.0 - (z / 30.0).powi(2)));
    for (fit, want) in [(RailFit::Stretch, 5000.0), (RailFit::Scale, 6400.0)] {
        let b = crate::loft_to_point_with_rails(&at(0.0), &square(10.0), apex, std::slice::from_ref(&r), fit).unwrap();
        assert!(close(volume(&b), want, 1e-3), "{fit:?}: volume {} vs {want}", volume(&b));
        let m = measure(&b).unwrap();
        assert!((m.bbox.max.y - 10.0).abs() < 1e-6 && (m.bbox.max.z - 30.0).abs() < 1e-6, "{fit:?}: {:?}", m.bbox);
    }
}

/// A closed loop of two cubic Béziers lofted to a point along a straight rail from one of its
/// points to the apex: the rail lies on the cone over the loop, so the loft is that cone, the
/// loft to the point without rails (exact ruled walls), area × h / 3.
#[test]
fn a_spline_profile_follows_a_straight_rail_to_the_apex() {
    let apex = Vec3::new(0.0, 0.0, 30.0);
    let (a, b) = (Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0));
    let lp = Loop2 {
        segs: vec![
            Seg2::Cubic { p0: a, p1: Vec2::new(-10.0, 12.0), p2: Vec2::new(10.0, 12.0), p3: b },
            Seg2::Cubic { p0: b, p1: Vec2::new(10.0, -12.0), p2: Vec2::new(-10.0, -12.0), p3: a },
        ],
    };
    let r: Vec<Vec3> = (0..=60).map(|i| f64::from(i) / 60.0).map(|t| Vec3::new(10.0 * (1.0 - t), 0.0, 30.0 * t)).collect();
    let railed = crate::loft_to_point_with_rails(&at(0.0), &lp, apex, &[r], RailFit::Scale).unwrap();
    let plain = crate::loft_to_point(&at(0.0), &lp, apex).unwrap();
    let (v, want) = (volume(&railed), volume(&plain));
    assert!(close(v, want, 1e-6), "railed {v} vs plain {want}");
}
