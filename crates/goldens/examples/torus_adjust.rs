//! Reproduction for plan/findings/surface.torus--adjust-step-uv-domain-negative-v-start-fails-in-rust.md:
//! `AdjustSTEPUVDomain` on a torus over several STEP domains (Rust side only).
#![allow(clippy::unwrap_used)]
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::math::Vec3;
use solidcore::surface::surface::Surface;
use solidcore::surface::torus::{Torus, TorusOptions};
fn main() {
    let k = solidcore::kernel::Kernel::default();
    let f = Axis2Placement::new(
        Vec3::new(-190.5, -12.7, -952.5),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    for d in [
        [0.0, -270.0, 360.0, -90.0],
        [0.0, 90.0, 360.0, 270.0],
        [0.0, -90.0, 360.0, 90.0],
        [0.0, -180.0, 180.0, 0.0],
        [0.0, 180.0, 180.0, 360.0],
        [-180.0, 90.0, 0.0, 270.0],
        [180.0, 90.0, 360.0, 270.0],
        [0.0, -10.0, 90.0, 10.0],
        [0.0, -100.0, 90.0, -80.0],
    ] {
        let mut s = Surface::from(
            Torus::create_canonical(&k, &f, 177.8, 12.7, TorusOptions::default(), None).unwrap(),
        );
        println!(
            "{d:?} -> {:?}",
            s.adjust_step_uv_domain(&k, &Extent2d::from_bounds(d[0], d[1], d[2], d[3]))
        );
    }
}
