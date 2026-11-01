//! WR-701 (the fork's tangency-labs/smlib@0c00de7): `SmBSplineSurface::SwapUV` keeps an
//! analytic surface's definition as it was made (`SwapUVDefinition`) and doesn't re-derive
//! it from the transposed net.
//!
//! The case is the fork's capi `analytic_frames_survive_swap_uv`. A cone, a sphere, a
//! torus and a plane on one oblique frame 45 from the origin are each swapped once and
//! back. With the flag on, the frame (a plane's two axes swapped on each turn) and the
//! radii are exactly as made, and the net's points are where they were, u and v swapped,
//! to 1e-13. Upstream fits the frame again to the net, and on the fork's base it came back
//! 3.6e-15 to 2.7e-14 off.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)] // test helpers

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::math::{Point3, Vec3};
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::Kernel;

fn frame() -> Axis2Placement {
    let mut f = Axis2Placement::default();
    f.set_canonical(
        Point3::new(31.3, -24.7, 18.4),
        Vec3::new(1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0),
        Vec3::new(2.0 / 3.0, 1.0 / 3.0, -2.0 / 3.0),
    )
    .unwrap();
    f
}

/// The surface's frame and its radii (cone: bottom radius and semi-angle).
fn definition(s: &Surface) -> (Axis2Placement, [f64; 2]) {
    match s.kind() {
        SurfaceKind::Cone(c) => (*c.rev().position(), [c.bot_radius(), c.semi_angle_deg()]),
        SurfaceKind::Sphere(c) => (*c.base().position(), [c.radius(), 0.0]),
        SurfaceKind::Torus(c) => (*c.base().position(), [c.major_radius(), c.minor_radius()]),
        SurfaceKind::Plane(c) => (*c.position(), [0.0, 0.0]),
        _ => panic!("kind not used here"),
    }
}

fn frame_off(a: &Axis2Placement, b: &Axis2Placement, swapped_xy: bool) -> f64 {
    let (bx, by) = if swapped_xy {
        (b.y_axis(), b.x_axis())
    } else {
        (b.x_axis(), b.y_axis())
    };
    (a.origin() - b.origin())
        .length()
        .max((a.x_axis() - bx).length())
        .max((a.y_axis() - by).length())
}

/// Frame and radii off after one turn, after two, and the net's points off after one.
fn turn(k: &Kernel, mut s: Surface, plane: bool) -> (f64, f64, f64) {
    let copy = s.clone();
    let (f0, r0) = definition(&s);
    s.swap_uv(k).unwrap();
    let (f1, r1) = definition(&s);
    let d0 = copy.natural_uv_domain_with_nurb(k).unwrap();
    let d1 = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut net = 0.0_f64;
    for i in 0..=8 {
        for j in 0..=8 {
            let (a, b) = (f64::from(i) / 8.0, f64::from(j) / 8.0);
            let p = copy
                .evaluate_point(
                    k,
                    Point2::new(
                        d0.min().x + a * (d0.max().x - d0.min().x),
                        d0.min().y + b * (d0.max().y - d0.min().y),
                    ),
                )
                .unwrap();
            let q = s
                .evaluate_point(
                    k,
                    Point2::new(
                        d1.min().x + b * (d1.max().x - d1.min().x),
                        d1.min().y + a * (d1.max().y - d1.min().y),
                    ),
                )
                .unwrap();
            net = net.max((p - q).length());
        }
    }
    s.swap_uv(k).unwrap();
    let (f2, r2) = definition(&s);
    let once = frame_off(&f0, &f1, plane)
        .max((r0[0] - r1[0]).abs())
        .max((r0[1] - r1[1]).abs());
    let twice = frame_off(&f0, &f2, false)
        .max((r0[0] - r2[0]).abs())
        .max((r0[1] - r2[1]).abs());
    (once, twice, net)
}

fn adjusted(k: &Kernel, mut s: Surface, lo: (f64, f64), hi: (f64, f64)) -> Surface {
    s.adjust_step_uv_domain(k, &Extent2d::from_bounds(lo.0, lo.1, hi.0, hi.1))
        .unwrap();
    s
}

fn surfaces(k: &Kernel) -> Vec<(&'static str, Surface, bool)> {
    let f = frame();
    vec![
        (
            "cone",
            Surface::from(Cone::create_canonical(k, &f, 2.0, 2.4, 25.0, false, false).unwrap()),
            false,
        ),
        (
            "sphere",
            adjusted(
                k,
                Surface::from(Sphere::create_canonical(k, &f, 2.5).unwrap()),
                (0.0, 0.0),
                (70.0, 90.0),
            ),
            false,
        ),
        (
            "torus",
            Surface::from(
                Torus::create_canonical(
                    k,
                    &f,
                    4.0,
                    1.5,
                    TorusOptions::default(),
                    Some(&Extent2d::from_bounds(0.0, 0.0, 80.0, 120.0)),
                )
                .unwrap(),
            ),
            false,
        ),
        (
            "plane",
            adjusted(
                k,
                Surface::from(Plane::create_canonical(&f).unwrap()),
                (-3.0, -2.0),
                (5.0, 7.0),
            ),
            true,
        ),
    ]
}

#[test]
fn analytic_frames_survive_swap_uv() {
    let mut k = Kernel::default();
    k.fixes.swap_uv_keeps_analytic_definition = true;
    for (what, s, plane) in surfaces(&k) {
        let (once, twice, net) = turn(&k, s, plane);
        assert!(
            once == 0.0 && twice == 0.0 && net <= 1e-13,
            "{what}: frame and radii off by {once:e}, back by {twice:e}; net off by {net:e}"
        );
    }
}

#[test]
fn upstream_rederives_the_frames_from_the_net() {
    let k = Kernel::default();
    let moved: Vec<_> = surfaces(&k)
        .into_iter()
        .filter_map(|(what, s, plane)| {
            let (once, twice, _) = turn(&k, s, plane);
            (once > 0.0 || twice > 0.0).then_some(what)
        })
        .collect();
    // the fork's base moved every one of them
    assert_eq!(moved, ["cone", "sphere", "torus", "plane"]);
}
