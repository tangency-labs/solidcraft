//! tangency-labs/solidcraft#15559 (WR-780): after `SwapUV`, an offset surface past its
//! periodic direction is evaluated at the periodic image of the parameter.
//!
//! Upstream's `SmOffsetSurface::SwapUV` swaps the base surface's directions but keeps
//! `m_bIsPeriodicOnU`/`V` and `m_lSingularities`. `sm_ExtendSurface` wraps a parameter
//! by those flags, so after a swap it wrapped the wrong direction: an offset surface of
//! revolution evaluated past its angle domain was extended tangentially (goldens
//! `surface.offset_surface` gen0354: 43 off the surface) or, with WR-725, put on the
//! seam. `OffsetSurface::swap_uv` now swaps the flags and the singular sides with the
//! directions.
//!
//! Closed form: the offset of a line generator is the line parallel to it at the offset
//! distance, and the surface of revolution turns it about the axis. The full turn's NURB
//! is the rational quadratic circle in four quarters of its angle domain, so the
//! parameter `a + f (b - a)` lies at the angle of that circle at `f`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;

/// The generator: a line in the xz-plane from (2, 0, 0) to (2.5, 0, 2).
const A: [f64; 2] = [2.0, 0.0];
const B: [f64; 2] = [2.5, 2.0];
const D: f64 = 0.75;

/// The angle, in radians, of the four-quarter rational quadratic circle at `f` in [0, 1].
fn circle_angle(f: f64) -> f64 {
    let quarter = (f * 4.0).floor().min(3.0);
    let s = f * 4.0 - quarter;
    let w = core::f64::consts::FRAC_1_SQRT_2;
    let basis = [(1.0 - s) * (1.0 - s), 2.0 * s * (1.0 - s), s * s];
    let weights = [1.0, w, 1.0];
    let points = [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let (mut x, mut y, mut den) = (0.0, 0.0, 0.0);
    for ((bi, wi), (px, py)) in basis.iter().zip(weights).zip(points) {
        let c = bi * wi;
        den += c;
        x += c * px;
        y += c * py;
    }
    quarter * core::f64::consts::FRAC_PI_2 + (y / den).atan2(x / den)
}

/// The distance of `(r, z)` from the generator's line in the xz-plane.
fn from_generator(r: f64, z: f64) -> f64 {
    let (dx, dz) = (B[0] - A[0], B[1] - A[1]);
    ((r - A[0]) * dz - (z - A[1]) * dx).abs() / dx.hypot(dz)
}

fn swapped_offset(k: &Kernel) -> OffsetSurface {
    let start = Point3::new(A[0], 0.0, A[1]);
    let end = Point3::new(B[0], 0.0, B[1]);
    let line = Curve::from(create_line_segment(start, end).unwrap());
    let ivl = line.natural_interval();
    let revolution = SurfOfRevolution::new(
        k,
        line,
        Point3::ZERO,
        Vec3::X,
        Vec3::Y,
        Extent2d::from_bounds(0.0, ivl.min(), 360.0, ivl.max()),
        false,
    )
    .unwrap();
    let base = Surface::from(revolution);
    let mut off = OffsetSurface::new(k, D, base, true, SS_UNKNOWN).unwrap();
    off.set_allow_extension(true);
    let before = off.is_periodic();
    assert_eq!(before, [1, 0], "the angle is u before the swap");
    off.swap_uv(k).unwrap();
    assert_eq!(
        off.is_periodic(),
        [0, 1],
        "the periodic flag follows the angle to v"
    );
    off
}

#[test]
fn issue_15559_past_the_angle_domain_is_the_periodic_image() {
    let k = Kernel::default();
    let mut off = swapped_offset(&k);
    let dom = off.natural_uv_domain();
    let (a, b) = (dom.min().y, dom.max().y);
    let t = 0.5 * (dom.min().x + dom.max().x);
    let f = 0.569_519_494_251_942; // gen0354's 4.5695... less four periods
    let inside = Point2::new(t, a + f * (b - a));
    let past = Point2::new(t, a + (4.0 + f) * (b - a));
    let p_in = off.evaluate_point_extending(&k, inside).unwrap();
    let p_past = off.evaluate_point_extending(&k, past).unwrap();
    let scale = p_in.length().max(1.0);
    assert!(
        p_past.distance(p_in) <= 1e-12 * scale,
        "past {p_past:?}, periodic image {p_in:?}"
    );
    // on the offset surface: (r, z) at the offset distance from the generator's line
    let r = p_past.x.hypot(p_past.y);
    assert!(
        (from_generator(r, p_past.z) - D).abs() <= 1e-12 * scale,
        "(r, z) = ({r}, {}) is {} from the generator",
        p_past.z,
        from_generator(r, p_past.z)
    );
    // at the wrapped angle (about 204 degrees), not at the seam
    let angle = p_past.y.atan2(p_past.x).rem_euclid(core::f64::consts::TAU);
    let want = circle_angle(f);
    assert!(
        (angle - want).abs() <= 1e-12,
        "angle {} degrees, want {}",
        angle.to_degrees(),
        want.to_degrees()
    );
}

#[test]
fn issue_15559_swap_uv_twice_restores_the_flags() {
    let k = Kernel::default();
    let mut off = swapped_offset(&k);
    let sides = off.singularities();
    off.swap_uv(&k).unwrap();
    assert_eq!(off.is_periodic(), [1, 0]);
    off.swap_uv(&k).unwrap();
    assert_eq!(off.singularities(), sides);
}
