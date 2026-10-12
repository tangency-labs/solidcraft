//! End-to-end pipeline tests for `harness.selftest` (needs `--features cpp`).
#![cfg(feature = "cpp")]

use difftest::case::repo_root;
use difftest::{registry, run_unit, CaseContext, RunOptions};
use solidcore_ffi::units::harness_selftest as ffi;

fn ctx(seeds: usize) -> CaseContext {
    CaseContext {
        root: repo_root(),
        seeds,
    }
}

#[test]
fn cpp_side_is_real_smlib() {
    assert_eq!(ffi::ping(0.5).unwrap(), 2.0);
    // The NURBS Book ex. 2.3: N_{2,2}, N_{3,2}, N_{4,2} at u = 5/2 = 1/8, 6/8, 1/8.
    let r = ffi::basis_eval(&[0., 0., 0., 1., 2., 3., 4., 4., 5., 5., 5.], 2, 2.5, 1).unwrap();
    assert_eq!(r.span, 4);
    assert_eq!(r.basis, vec![0.125, 0.75, 0.125]);
    // Variable-length output through the SC_ERR_BUFFER retry path.
    assert_eq!(ffi::reverse(&[1.0, 2.0, 3.0]).unwrap(), vec![3.0, 2.0, 1.0]);
    assert_eq!(ffi::reverse(&[]).unwrap(), Vec::<f64>::new());
    // The safe wrapper rejects inputs that would be UB upstream.
    assert!(ffi::basis_eval(&[0., 1.], 2, 0.5, 1).is_err());
    assert!(ffi::basis_eval(&[0., 0., 1., 1.], 1, 0.5, 7).is_err());
}

#[test]
fn selftest_unit_passes() {
    let reg = registry();
    let unit = reg.iter().find(|u| u.id() == "harness.selftest").unwrap();
    let res = run_unit(unit.as_ref(), &ctx(64), &RunOptions::default());
    assert!(res.cases.len() >= 64 + 10, "cases: {}", res.cases.len());
    assert!(res.pass(), "{}", difftest::report::summary_line(&res));
}

/// Negative test: a 1e-8 relative perturbation of one Rust value per case
/// (10x the tolerance) must be caught in every case.
#[test]
fn perturbation_is_caught() {
    let reg = registry();
    let unit = reg.iter().find(|u| u.id() == "harness.selftest").unwrap();
    let opts = RunOptions {
        perturb: true,
        ..RunOptions::default()
    };
    let res = run_unit(unit.as_ref(), &ctx(64), &opts);
    let (perturbed, detected) = res.perturb_stats();
    assert!(perturbed > 64);
    assert_eq!(perturbed, detected);
    assert!(!res.pass());
    let c = res
        .cases
        .iter()
        .find(|c| c.perturbed == Some("value"))
        .unwrap();
    assert!(c.diff.max_rel_err > 9e-9 && c.diff.max_rel_err < 2e-8);
}
