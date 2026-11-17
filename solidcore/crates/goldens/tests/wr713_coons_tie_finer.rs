//! WR-713 regression (#18160): the Coons corner patch's refinement keeps a finer copy
//! whose largest side angle ties the coarser one's within its certified rounding bound.
//!
//! analytic028's largest angle sits at the patch corners, fixed by the shared loop
//! curves, so the bare bicubic patch and every refined copy read the same angle up to
//! about `1e-14`. The fork's strict `<` then kept the bare 4x4 patch (one span each way);
//! the tie rule takes the finest refined copy.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test

use goldens::case::repo_root;
use goldens::{registry, CaseContext};

#[test]
fn analytic028_takes_the_refined_patch() {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let unit = registry()
        .into_iter()
        .find(|u| u.id() == "nurbs.b_spline_surface_n_lib_p2")
        .expect("unit registered");
    let case = unit
        .cases(&ctx)
        .unwrap()
        .into_iter()
        .find(|c| c.name == "analytic028")
        .expect("case analytic028");
    let out = unit.run_rust(&case).unwrap();
    let u_knots = &out.values["patch0_u"];
    let v_knots = &out.values["patch0_v"];
    // the bare bicubic patch has 8 knots each way; a refined copy has 64 spans
    assert!(
        u_knots.len() > 8 && v_knots.len() > 8,
        "the bare Coons patch was kept: {} u knots, {} v knots",
        u_knots.len(),
        v_knots.len()
    );
}
