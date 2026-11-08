//! Regression (`#[ignore]`: about 3 minutes in release) for the solve work bound
//! `geom::global_solver::MAX_SOLVE_BRANCHES` (not upstream): a tiny-scale box / torus
//! intersection and difference (box (5e-5, -5e-5, -1e-4) 1e-4 x 1e-4 x 2e-4, torus at
//! the origin, major 1e-4, minor 3e-5). Each runs four large global solves; the largest
//! visits 3,408,795 branches (measured 2026-10-26 with the bound raised and the counts
//! logged). At the old default `2^20` the port refused them with its own `InvalidInput`;
//! the default `2^23` covers them, and this test checks both booleans finish. It asserts
//! only success, not the result geometry: the C++ reaches UB-175 on these inputs, so they
//! cannot be compared with it (`plan/findings/geom.global_solver--tiny-torus-c-reaches-ub175-2026-10-26.md`).
//! Run with `cargo test -p solidcore --release --test max_solve_branches_tiny_torus -- --ignored`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_torus};
use solidcore::Kernel;

#[test]
#[ignore = "slow: about 3 minutes in release (3.4 million solve branches)"]
fn max_solve_branches_tiny_torus_booleans_finish() {
    for (name, intersect) in [("intersection", true), ("difference", false)] {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::new(5e-5, -5e-5, -1e-4), 1e-4, 1e-4, 2e-4).unwrap();
        let t = create_torus(&mut k, Point3::new(0.0, 0.0, 0.0), 1e-4, 3e-5).unwrap();
        let r = if intersect {
            boolean_intersection(&mut k, b, t)
        } else {
            boolean_difference(&mut k, b, t)
        };
        if let Err(e) = r {
            panic!("tiny-scale box / torus {name} failed: {e}");
        }
    }
}
