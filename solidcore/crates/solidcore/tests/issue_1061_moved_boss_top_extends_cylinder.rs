//! tangency-labs/solidcore#1061: a local extended offset (`SmBrep::LocalOperation`, which
//! the fork's capi `smlib_offset_faces` reaches through its `SmOffsetFacesExecutive` for a
//! face with no blend on it) that moves a boss's top face up leaves the moved part short of
//! its closed-form volume. The faces stay exact (planes and the wall's cylinder); the
//! shortfall is the moved top's rim, the plane z = 22 meeting the cylinder, which
//! `MateEdges` marches as a B-spline 7.5e-7 off the circle (its trim on the wall a
//! degree 3 curve of 358 control points), so the top's disc loses a sliver: 9.7e-9 of the
//! volume. With `offset_analytic_mate_edges` (WR-495, the fork's tangency-labs/smlib@fddc366
//! and @98ab1b0) the rim is the exact circle and the volume is exact. Extending the wall's
//! cylinder as itself (WR-532, now the only extension) changes neither.
//!
//! The part: a 60 x 40 x 10 plate with an r10 boss on it at (30, 20), its top at z = 20,
//! moved to z = 22 by its own plane offset by 2 (`CreateOffsetSurface`, as the capi moves a
//! face): the plate plus a cylinder r10 from z = 10 to 22.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, Notifications};
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::{Brep, Kernel};

/// The volume at the integration tolerance 1e-9 (the unmoved part comes out 1.3e-14 off).
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The largest distance of the moved top's rim (the edge on the plane z = 22 and the
/// cylinder r10 about (30, 20)) from that circle, over 65 points.
fn rim_off_circle(k: &Kernel, b: &Brep) -> f64 {
    let mut rims = 0;
    let mut off: f64 = 0.0;
    for (_, rec) in b.edges().iter() {
        let Some(c) = rec.curve.as_ref() else {
            continue;
        };
        let pts: Vec<Point3> = (0..=64)
            .map(|i| {
                c.evaluate_point(k, rec.interval.evaluate(f64::from(i) / 64.0))
                    .unwrap()
            })
            .collect();
        let radius = |p: &Point3| (p.x - 30.0).hypot(p.y - 20.0);
        if pts
            .iter()
            .all(|p| (p.z - 22.0).abs() < 1e-3 && (radius(p) - 10.0).abs() < 1e-3)
        {
            rims += 1;
            for p in &pts {
                off = off.max((p.z - 22.0).abs()).max((radius(p) - 10.0).abs());
            }
        }
    }
    assert_eq!(rims, 1, "the moved top's rim");
    off
}

/// The moved part's volume relative to its closed form, `(got - want) / want`, and its
/// rim's distance from the circle.
fn moved_top(exact_mates: bool) -> (f64, f64) {
    let mut k = Kernel::default();
    k.fixes.offset_analytic_mate_edges = exact_mates;
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let boss = create_cylinder(&mut k, Point3::new(30.0, 20.0, 5.0), 10.0, 15.0).unwrap();
    let mut part = boolean_union(&mut k, plate, boss).unwrap();
    let top = planar_faces_at(&k, &part, 2, 20.0);
    assert_eq!(top.len(), 1, "the boss's top face");
    // the top's own plane offset 2 along its outward (+z) normal
    let face = part.face(top[0]).unwrap();
    let s = face.surface.as_ref().unwrap();
    let n = s
        .evaluate_normal(&k, face.uv_domain.mid(), Side::Left, Side::Left)
        .unwrap();
    let d = if n.z > 0.0 { 2.0 } else { -2.0 };
    let moved = s.create_offset_surface(&k, d, 1e-6).unwrap().unwrap();
    let mut notifications = Notifications::new(&mut part);
    local_operation(
        &mut k,
        &mut part,
        &top,
        vec![moved],
        &mut notifications.context(),
    )
    .unwrap();
    solidcore::topology::integrity::check(&part).unwrap();
    let want = 60.0 * 40.0 * 10.0 + PI * 100.0 * 12.0;
    let got = volume(&k, &mut part);
    let off = rim_off_circle(&k, &part);
    eprintln!("exact mates {exact_mates}: volume {got}, closed form {want}, rim off {off:e}");
    ((got - want) / want, off)
}

#[test]
fn issue_1061_moved_boss_top_is_exact_with_exact_mate_edges() {
    let (rel, off) = moved_top(true);
    // measured 1.2e-14 and 3.6e-15
    assert!(rel.abs() < 1e-12, "{rel}");
    assert!(off < 1e-12, "{off}");
}

/// Without the opt-in the rim is marched off the circle and the part comes out short
/// (measured -9.7e-9 and 7.5e-7), as upstream marches it.
#[test]
fn issue_1061_without_the_opt_in_the_rim_is_marched() {
    let (rel, off) = moved_top(false);
    assert!(rel < -1e-9, "{rel}");
    assert!(off > 1e-8, "{off}");
}

/// **tangency-labs/solidcraft#16727 (a face turned about a hinge through both ends of one of its
/// edges).** A ring between the cylinder r 18 and an inner wall about the vertical axis through
/// (`cx`, 0), z from 0 to 9, cut to x >= 0. The inner wall is the cone
/// r(z) = 15 + (12 - z) tan `cone` (a cylinder r 15 for `cone` = 0). Its flat bottom is drafted
/// -3 degrees about the neutral plane x = 0 (pull -x), so it turns about the y axis. With the
/// ring 2 past the hinge, the bottom's inner arc is less than a half circle and both its ends lie
/// on the hinge: they stay where they are while the arc between them moves (the turned plane meets
/// the cone in another curve).
///
/// The local operation's edge replacement (`SmOffsetGeometryCreation::ReplaceEdgeCurves`) took
/// ends that agree for "no change" and kept the arc's old curve, so the edge came out in two
/// versions and the result was refused. With the inner wall a cylinder the new edge is the old one
/// (a cylinder's sections by planes through a line square to its axis all lie on it the same way:
/// the arc's ends and the arc stay), and with the ring centred on the hinge the old boundary
/// happens to be right: those controls build either way.
mod hinge_through_an_edges_ends {
    use super::offset_fixture::planar_faces_at;
    use super::volume;
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::kernel::UpstreamFixes;
    use solidcore::math::{Point3, Vec3};
    use solidcore::offset::draft::{draft_surface, Draft};
    use solidcore::offset::offset_executive::p2::local_operation_simple;
    use solidcore::primitive::api_primitives::{create_box, create_cone, create_cylinder};
    use solidcore::topology::brep::p2::is_manifold_solid;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    use solidcore::{Brep, Kernel};

    const OUTER: f64 = 18.0;
    const HEIGHT: f64 = 9.0;
    const DRAFT: f64 = 3.0;

    /// SolidCraft's fix set: the corpus's `fixes.json`.
    fn solidcraft_kernel() -> Kernel {
        let names: Vec<String> =
            serde_json::from_str(include_str!("kernel_cases/fixes.json")).unwrap();
        let mut f = UpstreamFixes::default();
        for name in &names {
            *f.flag_mut(name).unwrap_or_else(|| panic!("flag {name}")) = true;
        }
        Kernel {
            fixes: f,
            ..Kernel::default()
        }
    }

    /// The inner wall's radius at height `z`.
    fn inner(cone: f64, z: f64) -> f64 {
        15.0 + (12.0 - z) * cone.to_radians().tan()
    }

    /// The ring about (`cx`, 0), its inner wall a cone of half angle `cone` degrees, cut to x >= 0.
    fn ring(k: &mut Kernel, cx: f64, cone: f64) -> Brep {
        let outer = create_cylinder(k, Point3::new(cx, 0.0, 0.0), OUTER, HEIGHT).unwrap();
        let wall = if cone == 0.0 {
            create_cylinder(k, Point3::new(cx, 0.0, -2.0), 15.0, 14.0).unwrap()
        } else {
            create_cone(
                k,
                Point3::new(cx, 0.0, -2.0),
                inner(cone, -2.0),
                inner(cone, 12.0),
                14.0,
            )
            .unwrap()
        };
        let ring = boolean_difference(k, outer, wall).unwrap();
        let left = create_box(k, Point3::new(-40.0, -30.0, -1.0), 40.0, 60.0, 13.0).unwrap();
        let mut b = boolean_difference(k, ring, left).unwrap();
        b.refresh_cached_topology();
        b
    }

    /// The ring's bottom drafted -3 degrees about x = 0: its volume, the body checked a valid
    /// manifold solid, or the refusal.
    fn drafted(cx: f64, cone: f64) -> Result<f64, String> {
        let mut k = solidcraft_kernel();
        let mut b = ring(&mut k, cx, cone);
        let bottom = planar_faces_at(&k, &b, 2, 0.0);
        assert_eq!(bottom.len(), 1, "one bottom face");
        let draft = Draft::new(
            Point3::new(0.0, 0.0, 4.5),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            -DRAFT.to_radians(),
        )
        .unwrap();
        let surface = draft_surface(&k, &b, bottom[0], &draft)
            .unwrap()
            .unwrap_or_else(|r| panic!("not drafted: {r:?}"));
        local_operation_simple(&mut k, &mut b, &bottom, vec![surface])
            .map_err(|e| e.to_string())?;
        b.refresh_cached_topology();
        if !is_manifold_solid(&b).unwrap() {
            return Err("not a manifold solid".into());
        }
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        if !check.is_valid() {
            return Err(format!("check: {check:?}"));
        }
        Ok(volume(&k, &mut b))
    }

    /// The area of the disc of radius `r` about (`cx`, 0) between the lines x = `a` and x = `b`.
    fn disc_strip(r: f64, cx: f64, a: f64, b: f64) -> f64 {
        // the antiderivative of the chord length 2 sqrt(r^2 - u^2), u = x - cx, clamped to the disc
        let f = |x: f64| {
            let u = (x - cx).clamp(-r, r);
            u * (r * r - u * u).max(0.0).sqrt() + r * r * (u / r).asin()
        };
        if b <= a {
            0.0
        } else {
            f(b) - f(a)
        }
    }

    /// The closed form: the turned bottom is the plane z = x tan 3 deg, so at height z the
    /// solid's section is the ring's between x = 0 and x = z / tan 3 deg. Its area is the outer
    /// disc's strip less the inner one's (closed form), integrated over z by Simpson's rule on
    /// 20000 panels (the area is smooth in z, the rule's error far below 1e-9 of the volume).
    fn closed_form(cx: f64, cone: f64) -> f64 {
        let t = DRAFT.to_radians().tan();
        let area = |z: f64| {
            let x1 = z / t;
            disc_strip(OUTER, cx, 0.0, x1) - disc_strip(inner(cone, z), cx, 0.0, x1)
        };
        let n = 20_000_u32;
        let h = HEIGHT / f64::from(n);
        let mut s = area(0.0) + area(HEIGHT);
        for i in 1..n {
            let w = if i % 2 == 1 { 4.0 } else { 2.0 };
            s += w * area(f64::from(i) * h);
        }
        s * h / 3.0
    }

    fn assert_closed_form(cx: f64, cone: f64) {
        let v = drafted(cx, cone).unwrap_or_else(|e| panic!("cx {cx} cone {cone}: {e}"));
        let want = closed_form(cx, cone);
        assert!(
            (v - want).abs() < 1e-6 * want,
            "cx {cx} cone {cone}: {v} vs {want}"
        );
    }

    /// The ring 2 past the hinge, a cone inside: the draft builds, at the closed form.
    #[test]
    fn a_bottom_drafted_about_its_inner_arcs_ends_beside_a_cone_closes() {
        assert_closed_form(2.0, 1.5);
    }

    /// The controls: the same ring with the inner wall a cylinder, and the cone ring centred on
    /// the hinge (its inner arc a half circle).
    #[test]
    fn the_cylinder_and_centred_controls_close() {
        assert_closed_form(2.0, 0.0);
        assert_closed_form(0.0, 1.5);
    }
}
