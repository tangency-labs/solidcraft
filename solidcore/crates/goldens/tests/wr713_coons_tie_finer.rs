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

fn run(name: &str) -> goldens::Output {
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
        .find(|c| c.name == name)
        .expect("the case");
    unit.run_rust(&case).unwrap()
}

/// The knot count of `key` (a block-summed array in the goldens, a plain one here).
fn knots(out: &goldens::Output, key: &str) -> usize {
    out.values[key].len()
}

/// analytic064 (a torus neighbour) keeps main's patch (149 by 143 knots). Its refined
/// copies' largest angles (0.51721 rad, `4.2e-6` along a side whose first span is
/// `4.9e-4` long) sat between the evenly spaced stations, which read every candidate as
/// the corner's 0.51715, so the tie took a finer copy (177 by 171 knots, the goldens line
/// of 3102961cd) that is farther from tangent there. The corner stations see the rise,
/// and the side normal is taken at the side's own point, so the bare patch is kept.
#[test]
fn analytic064_keeps_the_patch_its_refinements_do_not_beat() {
    let out = run("analytic064");
    assert_eq!(
        (knots(&out, "patch0_u"), knots(&out, "patch0_v")),
        (149, 143),
        "not main's patch"
    );
}

#[test]
fn analytic028_takes_the_refined_patch() {
    let out = run("analytic028");
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
