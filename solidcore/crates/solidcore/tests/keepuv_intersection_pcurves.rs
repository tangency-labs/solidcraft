//! #17896 (WR-361): `MakeFaceuse` takes the other operand's UV trims over only on that
//! operand's own edges, the ones `MergeEdge` copied. An edge the intersection made keeps
//! the trim `MakeFace` drops from the edge curve. The other face's UV curve on such an
//! edge is the intersector's approximation, and it can be the coarser of the two.
//!
//! The case is the `boolean.api_brep` fixture `issue383_seed4_box_difference_tangent_torus`:
//! a 7.5 x 7.5 x 27.5 box at (22.5, 25, 22.5), less the z-axis torus at (20, 22.5, 25)
//! with R 10 and r 2.5. The torus is tangent to the box's bottom. The torus face of the
//! result with v in [-pi/2, 0] lies inside x >= 22.5, y >= 25, x <= 30 and y <= 32.5.
//! Its exact area is the closed form below.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_with_options;
use solidcore::boolean::merge::MergeOptions;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::mass::integrator::properties::face_area;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_torus};
use solidcore::topology::brep::p1::faces;
use solidcore::Kernel;

/// The exact area of the face, as an integral over v of r (R + r cos v) times the length
/// of the u-interval the box's planes leave. That length is in closed form, through
/// acos/asin of 2.5 / rho and 10 / rho with rho = R + r cos v. The integral is taken by
/// 30-point Gauss-Legendre on 20 pieces between each pair of breaks (rho = 10 and
/// rho = sqrt(106.25), where the active planes change). Each piece's integrand is
/// analytic, and doubling the pieces leaves 24.322249641618495 unchanged to 1e-15.
const EXACT: f64 = 24.322_249_641_618_5;

/// The face's area on the pinned main (280783a7e), where every trim of the face is the
/// one `MakeFace` drops, at the same accuracy.
const MAIN: f64 = 24.322_247_256_570_044;

#[test]
fn intersection_edges_keep_the_dropped_trim_on_a_tangent_torus() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(22.5, 25.0, 22.5), 7.5, 7.5, 27.5).unwrap();
    let b = create_torus(&mut k, Point3::new(20.0, 22.5, 25.0), 10.0, 2.5).unwrap();
    let flags = MergeOptions::new(false, false, true, false, false);
    let op = BooleanOperation::from_code(3).unwrap();
    let mut out = boolean_with_options(&mut k, a, b, op, flags).unwrap();
    let r = &mut out.result;
    let mut best = f64::INFINITY;
    let mut area = f64::NAN;
    for f in faces(r, None).unwrap() {
        let a = face_area(&k, r, f, 1e-11).unwrap();
        if (a - EXACT).abs() < best {
            best = (a - EXACT).abs();
            area = a;
        }
    }
    // the copied intersector pcurves gave 24.322246954 (2.69e-6 short); the dropped
    // trims give main's 24.322247257 (2.39e-6 short)
    assert!(
        best <= (MAIN - EXACT).abs() * (1.0 + 1e-6),
        "the torus face's area {area} is {best:e} from the exact {EXACT}, farther than main's {MAIN}"
    );
}
