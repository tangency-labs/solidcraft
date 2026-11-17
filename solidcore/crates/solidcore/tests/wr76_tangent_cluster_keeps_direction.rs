//! WR-76: when a better tangent boundary start point replaces a cluster's representative
//! (`AddStartPoint`), it keeps the representative's direction.
//!
//! A sphere of radius 2 and a cylinder of radius 2 about the same axis touch along the
//! sphere's equator. The cylinder's tangent boundary samples give a cluster of start
//! points on the circle; the sign of each sample's direction depends only on the boundary
//! that produced it. Had the replacement flipped the cluster's direction, `TestPoint`
//! would snap a trace from the contact band's edge onto the cluster after one step (it
//! snaps onto a start point whose direction opposes the trace), the cluster would go
//! with that stub and the tangent circle would be lost. The intersection is the one
//! whole circle, in either operand order.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::intersect::api_intersectors::intersect_surfaces;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::surface::cone::Cone;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surface::Surface;

fn frame(origin: Vec3) -> Axis2Placement {
    let mut f = Axis2Placement::default();
    f.set_canonical(origin, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0))
        .unwrap();
    f
}

fn operands(k: &Kernel) -> (Surface, Surface) {
    let sphere =
        Surface::from(Sphere::create_canonical(k, &frame(Vec3::new(0.0, 0.0, 0.0)), 2.0).unwrap());
    let mut cylinder = Surface::from(
        Cone::create_canonical_cylinder(k, &frame(Vec3::new(0.0, 0.0, -3.0)), 2.0).unwrap(),
    );
    let mut d = Extent2d::default();
    d.set_min_max_coords(0.0, 0.0, 360.0, 6.0).unwrap();
    cylinder.adjust_step_uv_domain(k, &d).unwrap();
    (sphere, cylinder)
}

/// The one curve of the intersection is the whole equator.
fn assert_equator(k: &Kernel, s1: &Surface, s2: &Surface) {
    let (curves, status) = intersect_surfaces(k, s1, s2);
    status.unwrap();
    assert_eq!(curves.len(), 1, "expected the one tangent circle");
    let c = &curves[0];
    let d = c.natural_interval();
    let n = 256;
    let mut prev: Option<f64> = None;
    let mut turned = 0.0;
    for i in 0..=n {
        let p = c
            .evaluate_point(k, d.evaluate(f64::from(i) / f64::from(n)))
            .unwrap();
        // on both surfaces, within the intersector's tolerance (a thousandth of
        // the operands' size, which is over 4)
        let r = (p.x * p.x + p.y * p.y).sqrt();
        let rho = (r * r + p.z * p.z).sqrt();
        assert!(
            (r - 2.0).abs() <= 1e-3,
            "point {i} {r} from the cylinder axis"
        );
        assert!(
            (rho - 2.0).abs() <= 1e-3,
            "point {i} {rho} from the sphere centre"
        );
        let a = p.y.atan2(p.x);
        if let Some(b) = prev {
            let mut step = a - b;
            if step > std::f64::consts::PI {
                step -= std::f64::consts::TAU;
            } else if step < -std::f64::consts::PI {
                step += std::f64::consts::TAU;
            }
            turned += step;
        }
        prev = Some(a);
    }
    assert!(
        (turned.abs() - std::f64::consts::TAU).abs() <= 1e-6,
        "the curve turns {turned} rad about the axis, not one whole turn"
    );
}

#[test]
fn coaxial_sphere_cylinder_tangent_circle() {
    let k = Kernel::default();
    let (sphere, cylinder) = operands(&k);
    assert_equator(&k, &sphere, &cylinder);
}

#[test]
fn coaxial_sphere_cylinder_tangent_circle_swapped() {
    let k = Kernel::default();
    let (sphere, cylinder) = operands(&k);
    assert_equator(&k, &cylinder, &sphere);
}
