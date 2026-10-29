//! tangency-labs/solidcore#1453 (the cylinder form of #1130): a cylinder r5, h10
//! shelled 1 inward with its top cap open. `MateEdges` intersects the offset faces'
//! tangent (G1R) B-spline extensions, so every new inner edge between two analytic
//! faces is marched: the floor circle and the seam line are B-splines, the rim is split
//! at an extra vertex, and the volume is 3.5e-7 over the closed form.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use core::f64::consts::PI;

use offset_fixture::{planar_faces_at, shell};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::p2::{ELLIPSE_TYPE, LINE_TYPE};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{
    create_box, create_cone, create_cylinder, create_sphere,
};
use solidcore::topology::brep::p1;
use solidcore::{Brep, Kernel};

fn volume_at(k: &Kernel, b: &mut Brep, accuracy: f64) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs)
}

/// The inner edges (every sample within 1e-3 of `on`, which gives a point's distance
/// from the inner surface): (count, how many are circles/ellipses or lines, worst
/// sample distance).
fn inner_edges(k: &Kernel, b: &Brep, on: impl Fn(Point3) -> f64) -> (usize, usize, f64) {
    let (mut count, mut exact, mut worst) = (0, 0, 0.0_f64);
    for e in p1::edges(b, None).unwrap() {
        let edge = b.edge(e).unwrap();
        let Some(curve) = edge.curve.as_ref() else {
            continue;
        };
        let mut dev = 0.0_f64;
        for i in 0..=32 {
            let t = edge.interval.evaluate(f64::from(i) / 32.0);
            let p = curve.evaluate_point(k, t).unwrap();
            dev = dev.max(on(p).abs());
        }
        if dev < 1e-3 {
            count += 1;
            worst = worst.max(dev);
            let kind = curve.is_kind_of(ELLIPSE_TYPE) || curve.is_kind_of(LINE_TYPE);
            eprintln!(
                "PROBE issue_1453 inner edge {e:?}: type {} dev {dev:e}",
                curve.type_code()
            );
            exact += usize::from(kind);
        }
    }
    (count, exact, worst)
}

struct Shelled {
    edges: usize,
    vertices: usize,
    inner: usize,
    exact: usize,
    worst: f64,
    volume: Option<f64>,
}

/// `part` shelled `distance` with the plane face at z = `open_z` open, measured (the
/// volume only when `want` is given: integrating a marched cone takes minutes).
fn shelled(
    label: &str,
    fixed: bool,
    part: impl Fn(&mut Kernel) -> Brep,
    open_z: f64,
    distance: f64,
    on: impl Fn(Point3) -> f64,
    want: Option<f64>,
) -> Shelled {
    let mut k = Kernel::default();
    k.fixes.offset_analytic_mate_edges = fixed;
    let part = part(&mut k);
    let open = planar_faces_at(&k, &part, 2, open_z);
    assert_eq!(open.len(), 1, "one open face");
    let mut b = shell(&mut k, part, distance, &open).unwrap();
    solidcore::topology::integrity::check(&b).unwrap();
    let (inner, exact, worst) = inner_edges(&k, &b, on);
    let volume = want.map(|_| volume_at(&k, &mut b, 1e-12).unwrap());
    let r = Shelled {
        edges: b.edge_list().len(),
        vertices: b.vertex_list().len(),
        inner,
        exact,
        worst,
        volume,
    };
    eprintln!(
        "PROBE {label} fixed={fixed}: edges={} vertices={} inner={inner} exact={exact} worst={worst:e} volume={volume:?} want={want:?}",
        r.edges,
        r.vertices
    );
    r
}

fn cylinder(k: &mut Kernel) -> Brep {
    create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0).unwrap()
}

fn on_cylinder(p: Point3) -> f64 {
    p.x.hypot(p.y) - 4.0
}

/// The issue's case: a cylinder r5, h10 shelled 1 inward, its top open. By default
/// (upstream) the new inner edges are marched: 7 edges and 5 vertices (the rim split
/// at an extra vertex), the volume 3.5e-7 over. With the fix: the floor and rim are
/// exact circles, 6 edges and 4 vertices, and the volume is pi 25 10 - pi 16 9 to
/// 1e-9.
#[test]
fn issue_1453_shelled_cylinder_edges_are_exact() {
    let want = PI * 25.0 * 10.0 - PI * 16.0 * 9.0;
    let stock = shelled(
        "issue_1453 cylinder",
        false,
        cylinder,
        10.0,
        -1.0,
        on_cylinder,
        Some(want),
    );
    assert_eq!(
        (stock.edges, stock.vertices),
        (7, 5),
        "upstream's marched rim"
    );
    assert!(stock.exact < stock.inner, "a marched inner edge");
    let over = stock.volume.unwrap() - want;
    assert!(over > 1e-7 * want, "upstream's volume 3.5e-7 over: {over}");
    let fixed = shelled(
        "issue_1453 cylinder",
        true,
        cylinder,
        10.0,
        -1.0,
        on_cylinder,
        Some(want),
    );
    assert_eq!((fixed.edges, fixed.vertices), (6, 4), "no extra rim vertex");
    assert_eq!(fixed.inner, 3, "floor circle, seam, rim circle");
    // The floor and rim circles are exact. The seam, where the inner wall meets
    // itself, has no exact intersection of two analytic copies (one cylinder twice),
    // so it stays the marched B-spline, on the line x = 4, y = 0.
    assert_eq!(fixed.exact, 2, "floor and rim are circles");
    assert!(fixed.worst <= 1e-12, "inner edges on r 4: {}", fixed.worst);
    let got = fixed.volume.unwrap();
    assert!((got - want).abs() <= 1e-9 * want, "{got} vs {want}");
}

fn frustum(k: &mut Kernel) -> Brep {
    create_cone(k, Point3::new(0.0, 0.0, 0.0), 5.0, 3.0, 10.0).unwrap()
}

/// The frustum's inner wall: the cone moved sqrt(1.04) inward, from z = 1 to 10.
fn frustum_inner_radius(z: f64) -> f64 {
    5.0 - 0.2 * z - 1.04_f64.sqrt()
}

fn on_frustum(p: Point3) -> f64 {
    p.x.hypot(p.y) - frustum_inner_radius(p.z)
}

/// A frustum r5 to r3 over h10 shelled 1 inward, its top open. By default its floor
/// and rim are marched; with the fix they are exact circles on the inner cone.
#[test]
fn issue_1453_shelled_cone_edges_are_exact() {
    let stock = shelled(
        "issue_1453 cone",
        false,
        frustum,
        10.0,
        -1.0,
        on_frustum,
        None,
    );
    assert_eq!(stock.exact, 0, "upstream's marched floor and rim");
    let fixed = shelled(
        "issue_1453 cone",
        true,
        frustum,
        10.0,
        -1.0,
        on_frustum,
        None,
    );
    assert_eq!((fixed.edges, fixed.vertices), (6, 4));
    assert_eq!(fixed.exact, 2, "floor and rim are circles");
    assert!(
        fixed.worst <= 1e-12,
        "inner edges on the inner cone: {}",
        fixed.worst
    );
}

/// The fixed frustum shell's volume is the closed form to 1e-9 (2.2e-14 measured;
/// upstream's is 1.9e-7 over). Ignored by default: integrating the cone faces takes
/// about four minutes.
#[test]
#[ignore = "slow: about four minutes of volume integration"]
fn issue_1453_shelled_cone_volume_is_exact() {
    let (r1, r2) = (frustum_inner_radius(1.0), frustum_inner_radius(10.0));
    let outer = PI * 10.0 / 3.0 * (25.0 + 15.0 + 9.0);
    let want = outer - PI * 9.0 / 3.0 * (r1 * r1 + r1 * r2 + r2 * r2);
    let fixed = shelled(
        "issue_1453 cone",
        true,
        frustum,
        10.0,
        -1.0,
        on_frustum,
        Some(want),
    );
    let got = fixed.volume.unwrap();
    assert!((got - want).abs() <= 1e-9 * want, "{got} vs {want}");
}

/// #1130's dome (a sphere r20 above z = 0) shelled 2 inward, its base open: the rim
/// between the inner sphere and the base plane is the circle r18 with or without the
/// fix (the port does not march it, plan/findings/offset.offset_geometry_creation--issue-1130-not-reproduced-in-port.md);
/// the fix keeps it so.
#[test]
fn issue_1453_shelled_dome_rim_stays_exact() {
    let dome = |k: &mut Kernel| {
        let ball = create_sphere(k, Point3::new(0.0, 0.0, 0.0), 20.0).unwrap();
        let below = create_box(k, Point3::new(-30.0, -30.0, -30.0), 60.0, 60.0, 30.0).unwrap();
        boolean_difference(k, ball, below).unwrap()
    };
    let on = |p: Point3| (p.distance(Point3::new(0.0, 0.0, 0.0)) - 18.0).abs() + p.z.abs();
    let stock = shelled("issue_1453 dome", false, dome, 0.0, -2.0, on, None);
    let fixed = shelled("issue_1453 dome", true, dome, 0.0, -2.0, on, None);
    assert!(
        fixed.inner >= 1 && fixed.exact == fixed.inner,
        "the rim a circle"
    );
    assert!(fixed.worst <= 1e-12, "rim on r 18: {}", fixed.worst);
    assert_eq!((fixed.edges, fixed.vertices), (stock.edges, stock.vertices));
}
