//! WR-713 regression (#18160): the Coons corner patch's refinement keeps a finer copy
//! only when no side of it is worse than any copy held before.
//!
//! analytic028's largest angle sits at the patch corners, fixed by the shared loop
//! curves, so the bare bicubic patch and every refined copy agree there to well within
//! the rule's resolution, and no side is certainly worse: the fork's strict `<` kept the
//! bare 4x4 patch, the side-by-side rule takes the finest (64-span) copy. analytic064's
//! refined copies are certainly worse along two sides, so it keeps the bare patch.
//!
//! Both tests run the case with the kernel's Coons log on, check the logged choice, and
//! check that the output holds that candidate's patch bit for bit.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test

use goldens::case::repo_root;
use goldens::units::nurbs_b_spline_surface_n_lib_p2 as nlib;
use goldens::{registry, CaseContext, Output};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::{CoonsCandidateLog, CoonsTangencyLog};

/// The case's output and its Coons logs.
fn run(name: &str) -> (Output, Vec<CoonsTangencyLog>) {
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
    let k = Kernel::default();
    *k.coons_tangency_log.borrow_mut() = Some(Vec::new());
    let out = nlib::run_case_on(&k, &case).unwrap();
    let logs = k.coons_tangency_log.borrow().clone().unwrap();
    // the log changes no result
    let plain = unit.run_rust(&case).unwrap();
    let bits = |o: &Output| -> Vec<(String, Vec<u64>)> {
        o.values
            .iter()
            .map(|(key, v)| (key.clone(), v.iter().map(|x| x.to_bits()).collect()))
            .collect()
    };
    assert_eq!(bits(&out), bits(&plain), "the logged run differs");
    (out, logs)
}

/// A candidate's patch as the goldens write it: control points (x, y, z, w), then knots.
fn patch_bits(c: &CoonsCandidateLog) -> Vec<u64> {
    let nurb = c.patch.gw_nurb().unwrap();
    let mut v = Vec::new();
    for row in nurb.cpts() {
        for p in row {
            v.extend([p.x, p.y, p.z, p.w]);
        }
    }
    v.extend_from_slice(nurb.u_knots());
    v.extend_from_slice(nurb.v_knots());
    v.into_iter().map(f64::to_bits).collect()
}

/// The output's first patch, in the same order.
fn output_bits(out: &Output) -> Vec<u64> {
    ["patch0_cpts", "patch0_u", "patch0_v"]
        .iter()
        .flat_map(|k| out.values[*k].iter().map(|x| x.to_bits()))
        .collect()
}

/// The single refinement's choice, checked against the output.
fn chosen(name: &str) -> Option<usize> {
    let (out, logs) = run(name);
    assert_eq!(logs.len(), 1, "{name}: one corner patch refined");
    let log = &logs[0];
    let i = log.chosen.unwrap_or(0);
    assert_eq!(
        output_bits(&out),
        patch_bits(&log.candidates[i]),
        "{name}: the output is not candidate {i}"
    );
    log.chosen
}

#[test]
fn analytic028_takes_the_finest_refined_patch() {
    let (_, logs) = run("analytic028");
    let spans: Vec<usize> = logs[0].candidates.iter().map(|c| c.spans).collect();
    assert_eq!(spans, [0, 8, 16, 32, 64], "the candidates");
    assert_eq!(chosen("analytic028"), Some(4), "not the 64-span copy");
}

#[test]
fn analytic064_keeps_the_patch_its_refinements_do_not_beat() {
    assert_eq!(chosen("analytic064"), None, "not the bare patch");
}
