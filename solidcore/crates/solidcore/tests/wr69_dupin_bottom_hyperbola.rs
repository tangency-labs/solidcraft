//! WR-69: the conjugate Dupin branches must have the same two axis lengths.

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::adv_surface_intersector::{AdvSurfaceIntersector, NoSurfaceHandle};
use solidcore::kernel::Kernel;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;

#[test]
fn bottom_hyperbola_uses_other_principal_curvature_only_when_enabled() {
    for enabled in [false, true] {
        let mut kernel = Kernel::default();
        assert!(!kernel.fixes.dupin_bottom_hyperbola_axis);
        kernel.fixes.dupin_bottom_hyperbola_axis = enabled;
        let domain = Extent2d::from_bounds(-1.0, -1.0, 1.0, 1.0);
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let z = Vec3::new(0.0, 0.0, 1.0);
        let zero = Vec3::new(0.0, 0.0, 0.0);
        let surface = Surface::from(Plane::new(zero, x, y, Vec2::new(1.0, 1.0), &domain));
        let si: AdvSurfaceIntersector<'_, '_, NoSurfaceHandle> =
            AdvSurfaceIntersector::new(&kernel, &surface, &domain, &surface, &domain, false)
                .unwrap();
        for (kuu, kvv) in [(4.0, -1.0), (1.0, -9.0), (4.0, -4.0)] {
            for normal in [z, -z] {
                // At the origin of z = (kuu*u^2 + kvv*v^2)/2, the first
                // fundamental form is the identity and the curvatures are kuu, kvv.
                let mut eval = [[zero, y, z * kvv], [x, zero, zero], [z * kuu, zero, z]];
                let dupin = si.make_dupin(&mut eval, normal).unwrap();
                assert_eq!((dupin.above.len(), dupin.below.len()), (2, 2));
                let (right_left, top_bottom) = if dupin.prin_k1 > 0.0 {
                    (&dupin.above, &dupin.below)
                } else {
                    (&dupin.below, &dupin.above)
                };
                let a = 1.0 / dupin.prin_k1.abs().sqrt();
                let b = 1.0 / dupin.prin_k2.abs().sqrt();
                for conic in right_left {
                    assert_eq!((conic.a(), conic.b()), (a, b));
                }
                assert_eq!((top_bottom[0].a(), top_bottom[0].b()), (b, a));
                assert_eq!(top_bottom[1].a(), b);
                assert_eq!(top_bottom[1].b(), if enabled { a } else { b });
                assert_eq!(top_bottom[1].theta_rad(), -1.5 * std::f64::consts::PI);
            }
        }
    }
}
