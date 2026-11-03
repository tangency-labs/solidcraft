//! WR-669 (`fixes.fillet_exact_round_classes`, the SolidCraft fork's tangency-labs/smlib@b5f2867
//! and 7ac037e): a constant round is exact by one classification of its spine from its
//! faces' kinds and placement, in place of upstream's try-chain (planes and cones only,
//! cylinder then torus, each gated on the edge's shape).
//!
//! - A sphere is a quadric now: the concave round where a coaxial cylinder leaves a ball
//!   is the ball's arc turned about the axis (a surface of revolution of the exact volume),
//!   where upstream traces and fits it.
//! - The point class (two cylinders as wide as the ball, their axes meeting) is the ball's
//!   piece without `fillet_corner_ball_patch` (WR-420), where upstream fails 1001.
//! - Rounds the try-chain already made exact (a box edge's cylinder, a plate's rim on a
//!   boss's torus) come out as before.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::primitive::api_surface_ops::create_offset_surface;
use solidcore::surface::procedural_blend::convert::{exact_rebuild, exact_to_record};
use solidcore::surface::procedural_blend::exact::ExactClass;
use solidcore::surface::procedural_blend::record::SupportRecord;
use solidcore::surface::procedural_blend::BlendNet;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    volume_to(k, b, 1e-12)
}

fn volume_to(k: &Kernel, b: &mut Brep, accuracy: f64) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs).unwrap()
}

fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

fn edges_where(k: &Kernel, b: &Brep, keep: impl Fn(Point3) -> bool) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    edges
        .into_iter()
        .filter(|&e| keep(midpoint(k, b, e)))
        .collect()
}

/// The number of faces on a surface of revolution (or torus).
fn revolved_faces(b: &Brep) -> usize {
    b.faces()
        .ids()
        .filter(|&f| {
            matches!(
                face::surface(b, f)
                    .unwrap()
                    .map(solidcore::surface::surface::Surface::kind),
                Some(SurfaceKind::SurfOfRevolution(_) | SurfaceKind::Torus(_))
            )
        })
        .count()
}

/// The r5 ball about the origin with the r2 cylinder up the z axis from z = 0 to 10
/// standing out of it, the concave edge where they meet (z = sqrt 21) rounded r1.
fn ball_and_peg(exact: bool) -> (Kernel, Brep, f64, solidcore::Result<()>) {
    ball_and_peg_kept(exact, false)
}

/// [`ball_and_peg`], with `definition` (`fillet_round_as_definition`, #2485) set.
fn ball_and_peg_kept(exact: bool, definition: bool) -> (Kernel, Brep, f64, solidcore::Result<()>) {
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = exact;
    k.fixes.fillet_round_as_definition = definition;
    let ball = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0).unwrap();
    let peg = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 2.0, 10.0).unwrap();
    let mut b = boolean_union(&mut k, ball, peg).unwrap();
    let before = volume_to(&k, &mut b, 1e-7);
    let rim = edges_where(&k, &b, |m| (m.z - 21f64.sqrt()).abs() < 1e-6);
    assert_eq!(rim.len(), 1, "the rim edge");
    let r = fillet_edges(&mut k, &mut b, &rim, 1.0, 1, 1, 1.0);
    (k, b, before, r)
}

/// The volume the r1 round adds between the r2 cylinder and the r5 ball: the ball of
/// radius 1 centred 3 from the axis at height zc = sqrt 27 touches the cylinder at
/// rho = 2 and the ball at rho = 2.5; the region between the sphere and the ball's arc,
/// turned about the axis: `2 pi int_2^2.5 rho (zb(rho) - zs(rho)) drho`.
fn ball_and_peg_round_volume() -> f64 {
    let zc = 27f64.sqrt();
    // int rho (zc - sqrt(1 - (rho - 3)^2)) drho, u = rho - 3
    let arc = |u: f64| {
        let w = 1.0 - u * u;
        -(w * w.sqrt()) / 3.0 + 3.0 * (u * w.sqrt() + u.asin()) / 2.0
    };
    let ball_part = zc * (2.5f64 * 2.5 - 2.0 * 2.0) / 2.0 - (arc(-0.5) - arc(-1.0));
    // int rho sqrt(25 - rho^2) drho
    let sphere = |rho: f64| -(25.0 - rho * rho).powf(1.5) / 3.0;
    let sphere_part = sphere(2.5) - sphere(2.0);
    2.0 * PI * (ball_part - sphere_part)
}

/// The largest distance, over a 21 x 21 grid of each revolved face's natural domain,
/// between the ball's radius 1 and the distance from the spine (the circle rho = 3,
/// z = sqrt 27 the ball's centre runs on).
fn revolved_off_spine(k: &Kernel, b: &Brep) -> f64 {
    let zc = 27f64.sqrt();
    let mut worst = 0.0f64;
    for f in b.faces().ids() {
        let Some(s) = face::surface(b, f).unwrap() else {
            continue;
        };
        if !matches!(
            s.kind(),
            SurfaceKind::SurfOfRevolution(_) | SurfaceKind::Torus(_)
        ) {
            continue;
        }
        let d = s.natural_uv_domain_with_nurb(k).unwrap();
        for i in 0..=20 {
            for j in 0..=20 {
                let u = d.min().x + (d.max().x - d.min().x) * f64::from(i) / 20.0;
                let v = d.min().y + (d.max().y - d.min().y) * f64::from(j) / 20.0;
                let p = s
                    .evaluate_point(k, solidcore::geom::vector2d::Point2::new(u, v))
                    .unwrap();
                let rho = (p.x * p.x + p.y * p.y).sqrt();
                let off = ((rho - 3.0).powi(2) + (p.z - zc).powi(2)).sqrt() - 1.0;
                worst = worst.max(off.abs());
            }
        }
    }
    worst
}

#[test]
fn a_round_between_a_ball_and_a_coaxial_peg_is_revolved_and_exact() {
    let (k, mut b, before, r) = ball_and_peg(true);
    r.unwrap();
    assert_eq!(
        revolved_faces(&b),
        1,
        "the round is a surface of revolution"
    );
    let off = revolved_off_spine(&k, &b);
    assert!(off < 1e-12, "the round is off the ball by {off:e}");
    let after = volume_to(&k, &mut b, 1e-7);
    let added = ball_and_peg_round_volume();
    assert!(
        (after - (before + added)).abs() < 1e-6 * after,
        "{before} + {added} != {after} (off {:e})",
        after - before - added
    );
}

#[test]
fn upstream_traces_the_round_between_a_ball_and_a_peg() {
    let (_, b, _, r) = ball_and_peg(false);
    r.unwrap();
    assert_eq!(revolved_faces(&b), 0, "upstream fits the traced round");
}

/// A 25 x 25 x 10 tray, bottom edges R2, then the vertical edges R2 (the corner ball where
/// two bottom rounds meet: the point class) as in tangency-labs/solidcore#416.
fn tray(exact: bool) -> (f64, f64, solidcore::Result<()>) {
    let (rounded, after, r, _, _) = tray_kept(exact, false);
    (rounded, after, r)
}

/// [`tray`], with `definition` (`fillet_round_as_definition`, #2485) set; also the kernel
/// and the tray.
fn tray_kept(exact: bool, definition: bool) -> (f64, f64, solidcore::Result<()>, Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = exact;
    k.fixes.fillet_round_as_definition = definition;
    let mut tray = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 25.0, 25.0, 10.0).unwrap();
    let bottom = edges_where(&k, &tray, |m| m.z.abs() < 1e-6);
    fillet_edges(&mut k, &mut tray, &bottom, 2.0, 1, 1, 1.0).unwrap();
    let rounded = volume(&k, &mut tray);
    let near_side = |v: f64, side: f64| (v.min(side - v) > 0.1) && (v.min(side - v) < 1.9);
    let vertical = edges_where(&k, &tray, |m| {
        (m.z - 6.0).abs() < 1e-6
            || (m.z > 0.1 && m.z < 1.9 && near_side(m.x, 25.0) && near_side(m.y, 25.0))
    });
    let r = fillet_edges(&mut k, &mut tray, &vertical, 2.0, 1, 1, 1.0);
    let after = volume(&k, &mut tray);
    (rounded, after, r, k, tray)
}

#[test]
fn the_point_class_makes_the_corner_ball_without_the_ball_patch_flag() {
    // each vertical edge loses (1 - pi/4) 4 along 8, each corner the octant of the two
    // bottom rounds' crossing (16/3) less the corner ball's (4 pi / 3)
    let removed = 4.0 * (32.0 * (1.0 - PI / 4.0) + 16.0 / 3.0 - 4.0 * PI / 3.0);
    let (rounded, after, r) = tray(true);
    r.unwrap();
    assert!(
        (after - (rounded - removed)).abs() < 1e-7 * after,
        "{rounded} {after}"
    );
    let (_, _, r) = tray(false);
    assert!(
        matches!(r, Err(solidcore::SolidError::Upstream { code: 1001, .. })),
        "{r:?}"
    );
}

/// A 20 x 20 x 4 plate with an r3 boss 6 high at its centre, the boss's foot (plane square
/// to the cylinder's axis: the circle class) and one plate edge (two planes: the line
/// class) rounded r1.
fn plate_and_boss(exact: bool) -> (f64, usize) {
    let (k, mut b) = plate_and_boss_kept(exact, false);
    (volume(&k, &mut b), revolved_faces(&b))
}

/// [`plate_and_boss`]'s body, with `definition` (`fillet_round_as_definition`, #2485) set.
fn plate_and_boss_kept(exact: bool, definition: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = exact;
    k.fixes.fillet_round_as_definition = definition;
    let plate = create_box(&mut k, Point3::new(-10.0, -10.0, 0.0), 20.0, 20.0, 4.0).unwrap();
    let boss = create_cylinder(&mut k, Point3::new(0.0, 0.0, 2.0), 3.0, 8.0).unwrap();
    let mut b = boolean_union(&mut k, plate, boss).unwrap();
    let foot = edges_where(&k, &b, |m| {
        (m.z - 4.0).abs() < 1e-6 && ((m.x * m.x + m.y * m.y).sqrt() - 3.0).abs() < 1e-6
    });
    assert!(!foot.is_empty());
    fillet_edges(&mut k, &mut b, &foot, 1.0, 1, 1, 1.0).unwrap();
    let side = edges_where(&k, &b, |m| {
        (m.z - 4.0).abs() < 1e-6 && (m.x - 10.0).abs() < 1e-6
    });
    assert_eq!(side.len(), 1);
    fillet_edges(&mut k, &mut b, &side, 1.0, 1, 1, 1.0).unwrap();
    (k, b)
}

#[test]
fn line_and_circle_rounds_the_chain_made_exact_are_unchanged() {
    let (exact, n_exact) = plate_and_boss(true);
    let (chain, n_chain) = plate_and_boss(false);
    // plate 1600 + boss 36 pi 6; the foot's round adds 2 pi int_3^4 rho (1 - sqrt(1 -
    // (rho - 4)^2)) drho = 2 pi (7/2 - (4 pi / 4 - 1/3)); the plate edge's takes 20 (1 - pi/4)
    let foot = 2.0 * PI * (3.5 - (PI - 1.0 / 3.0));
    let expected = 1600.0 + 54.0 * PI + foot - 20.0 * (1.0 - PI / 4.0);
    assert!(
        (exact - expected).abs() < 1e-9 * expected,
        "{exact} != {expected}"
    );
    assert!((exact - chain).abs() < 1e-9 * exact, "{exact} != {chain}");
    assert_eq!((n_exact, n_chain), (1, 1));
}

// ---- #2485: an exact round is a blend by its definition, its net the exact surface ----

/// The faces of `b` that are exact rounds, with their classes.
fn exact_rounds(b: &Brep) -> Vec<(solidcore::topology::FaceId, ExactClass)> {
    b.faces()
        .ids()
        .filter_map(|f| {
            let s = face::surface(b, f).unwrap()?;
            let blend = s.procedural_blend()?;
            Some((f, blend.exact()?.class()))
        })
        .collect()
}

/// Every exact round of `b` is a blend by its definition with a bound of 0, re-certified;
/// the classes found.
fn assert_definitions(k: &Kernel, b: &Brep) -> Vec<ExactClass> {
    let mut classes = Vec::new();
    for (f, class) in exact_rounds(b) {
        let s = face::surface(b, f).unwrap().unwrap();
        let blend = s.procedural_blend().unwrap();
        assert!(matches!(blend.net(), BlendNet::Exact(_)));
        assert_eq!(blend.bound(), 0.0);
        assert_eq!(blend.order(), 1);
        assert!(blend.support(0).is_some() && blend.support(1).is_some());
        assert_eq!(blend.recertify(k).unwrap(), 0.0, "{class:?}");
        classes.push(class);
    }
    classes
}

/// The points of every face's surface over a 9 x 9 grid of its natural domain, as bits.
fn face_bits(k: &Kernel, b: &Brep) -> Vec<(u8, Vec<[u64; 3]>)> {
    b.faces()
        .ids()
        .filter_map(|f| face::surface(b, f).unwrap())
        .map(|s| {
            let tag = match s.kind() {
                SurfaceKind::Cone(_) => 1,
                SurfaceKind::SurfOfRevolution(_) => 2,
                SurfaceKind::Torus(_) => 3,
                SurfaceKind::Sphere(_) => 4,
                SurfaceKind::BallPatch(_) => 5,
                SurfaceKind::Plane(_) => 6,
                SurfaceKind::BSpline(_) => 7,
                _ => 0,
            };
            let d = s.natural_uv_domain_with_nurb(k).unwrap();
            let mut pts = Vec::new();
            for i in 0..=8 {
                for j in 0..=8 {
                    let u = d.min().x + (d.max().x - d.min().x) * f64::from(i) / 8.0;
                    let v = d.min().y + (d.max().y - d.min().y) * f64::from(j) / 8.0;
                    let p = s
                        .evaluate_point(k, solidcore::geom::vector2d::Point2::new(u, v))
                        .unwrap();
                    pts.push([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
                }
            }
            (tag, pts)
        })
        .collect()
}

/// Every exact round of `b` through its record and back: the record re-serialised from the
/// rebuilt round is the record; the round is re-certified and of its kind again.
fn assert_records_round_trip(k: &Kernel, b: &Brep) -> usize {
    let mut n = 0;
    for (f, _) in exact_rounds(b) {
        let s = face::surface(b, f).unwrap().unwrap();
        let rec = exact_to_record(k, s).unwrap();
        let json = serde_json::to_string(&rec).unwrap();
        let back = exact_rebuild(k, &serde_json::from_str(&json).unwrap()).unwrap();
        assert_eq!(exact_to_record(k, &back).unwrap(), rec);
        assert_eq!(
            std::mem::discriminant(back.kind()),
            std::mem::discriminant(s.kind())
        );
        assert_eq!(back.procedural_blend().unwrap().bound(), 0.0);
        // the rebuilt net and supports evaluate as the round's, to rounding at part scale
        let (round, again) = (
            s.procedural_blend().unwrap(),
            back.procedural_blend().unwrap(),
        );
        assert_same_points(k, s, &back, "the net");
        for i in 0..2 {
            assert_same_points(
                k,
                round.support(i).unwrap(),
                again.support(i).unwrap(),
                "a support",
            );
        }
        n += 1;
    }
    n
}

/// `a` and `b` evaluate alike over a 9 x 9 grid of `a`'s natural domain, to the rounding of
/// the part (64 ulps of its size, 25).
fn assert_same_points(
    k: &Kernel,
    a: &solidcore::surface::surface::Surface,
    b: &solidcore::surface::surface::Surface,
    what: &str,
) {
    let d = a.natural_uv_domain_with_nurb(k).unwrap();
    let tol = 64.0 * f64::EPSILON * 25.0;
    for i in 0..=8 {
        for j in 0..=8 {
            let uv = solidcore::geom::vector2d::Point2::new(
                d.min().x + (d.max().x - d.min().x) * f64::from(i) / 8.0,
                d.min().y + (d.max().y - d.min().y) * f64::from(j) / 8.0,
            );
            let off =
                (a.evaluate_point(k, uv).unwrap() - b.evaluate_point(k, uv).unwrap()).length();
            assert!(
                off <= tol,
                "{what} rebuilt from its record is {off:e} off at {uv:?}"
            );
        }
    }
}

#[test]
fn the_line_and_circle_rounds_are_their_definitions() {
    let (k, mut b) = plate_and_boss_kept(true, true);
    let mut classes = assert_definitions(&k, &b);
    classes.sort_by_key(|c| c.name());
    assert_eq!(classes, vec![ExactClass::Circle, ExactClass::Line]);
    // the volume of the closed form, as without the definition
    let foot = 2.0 * PI * (3.5 - (PI - 1.0 / 3.0));
    let expected = 1600.0 + 54.0 * PI + foot - 20.0 * (1.0 - PI / 4.0);
    let v = volume(&k, &mut b);
    assert!((v - expected).abs() < 1e-9 * expected, "{v} != {expected}");
    // every surface bit for bit as without the definition
    let (k0, b0) = plate_and_boss_kept(true, false);
    assert!(exact_rounds(&b0).is_empty());
    assert_eq!(face_bits(&k, &b), face_bits(&k0, &b0));
    assert_eq!(assert_records_round_trip(&k, &b), 2);
}

#[test]
fn the_revolved_round_on_a_ball_is_its_definition() {
    let (k, mut b, before, r) = ball_and_peg_kept(true, true);
    r.unwrap();
    assert_eq!(assert_definitions(&k, &b), vec![ExactClass::Circle]);
    let after = volume_to(&k, &mut b, 1e-7);
    let added = ball_and_peg_round_volume();
    assert!((after - (before + added)).abs() < 1e-6 * after);
    let (k0, b0, _, r0) = ball_and_peg_kept(true, false);
    r0.unwrap();
    assert_eq!(face_bits(&k, &b), face_bits(&k0, &b0));
    assert_eq!(assert_records_round_trip(&k, &b), 1);
}

#[test]
fn the_corner_balls_are_their_definitions() {
    let removed = 4.0 * (32.0 * (1.0 - PI / 4.0) + 16.0 / 3.0 - 4.0 * PI / 3.0);
    let (rounded, after, r, k, b) = tray_kept(true, true);
    r.unwrap();
    assert!((after - (rounded - removed)).abs() < 1e-7 * after);
    let classes = assert_definitions(&k, &b);
    assert!(classes.contains(&ExactClass::Point), "{classes:?}");
    let (_, _, r0, k0, b0) = tray_kept(true, false);
    r0.unwrap();
    assert_eq!(face_bits(&k, &b), face_bits(&k0, &b0));
    assert_eq!(assert_records_round_trip(&k, &b), classes.len());
}

#[test]
fn the_offset_of_a_line_round_is_an_exact_cylinder() {
    let (k, b) = plate_and_boss_kept(true, true);
    let (f, _) = exact_rounds(&b)
        .into_iter()
        .find(|(_, c)| *c == ExactClass::Line)
        .unwrap();
    let s = face::surface(&b, f).unwrap().unwrap();
    for d in [0.25, -0.25] {
        let off = create_offset_surface(&k, s, d).unwrap();
        let SurfaceKind::Cone(c) = off.kind() else {
            panic!("the offset is a {:?}", off.kind());
        };
        assert!(c.is_cylinder());
        // the round's radius 1 moved by d, to rounding
        assert!(
            ((c.base_radius() - 1.0).abs() - 0.25).abs() <= 1e-14,
            "the offset's radius {}",
            c.base_radius()
        );
    }
}

#[test]
fn a_moved_exact_round_is_certified_again() {
    let (k, b) = plate_and_boss_kept(true, true);
    for (f, _) in exact_rounds(&b) {
        let mut s = face::surface(&b, f).unwrap().unwrap().copy(&k).unwrap();
        let mut place = solidcore::geom::axis2_placement::Axis2Placement::default();
        place.rotate_about_axis(
            0.3,
            solidcore::math::Vec3::new(1.0, 2.0, 3.0).unitize().unwrap(),
        );
        place.translate(solidcore::math::Vec3::new(1e3, -2e3, 5e2));
        s.transform(&k, &place, None).unwrap();
        assert_eq!(s.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
        // a support moved apart from the net is refused
        let mut off = exact_to_record(&k, &s).unwrap();
        if let SupportRecord::Nurbs { form, .. } = &mut off.supports[0] {
            for p in &mut form.points {
                for x in p.iter_mut() {
                    *x += 1e-3;
                }
            }
        }
        assert!(exact_rebuild(&k, &off).is_err());
    }
}
