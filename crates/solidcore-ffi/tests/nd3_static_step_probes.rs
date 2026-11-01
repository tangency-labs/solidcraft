//! ND-3: on a process that latched nothing, the two `sdStaticStep` probes read unlatched
//! (`1e-5` for the plain one, `1e-3` for the one that first makes a tangent-point
//! `ComputePointValues`), so `check_parent_static_step` can tell a clean parent from a
//! latched one. Its own test binary: no other test runs C++ in this process first.

#![cfg(feature = "cpp")]

use solidcore_ffi::isolate::fresh_process;
use solidcore_ffi::units::blend_fillet_solver_p4::{
    check_parent_static_step, static_step, static_step_after_tangent,
};

#[test]
fn a_clean_process_reads_both_probes_unlatched() {
    let plain = fresh_process(|| Ok(vec![static_step()?])).expect("plain probe");
    let tangent = fresh_process(|| Ok(vec![static_step_after_tangent()?])).expect("tangent probe");
    eprintln!("ND-3 probes on a clean process: plain {plain:?}, after a tangent point {tangent:?}");
    assert_eq!(plain, [1.0e-5], "the plain probe latches 1e-5");
    assert_eq!(tangent, [1.0e-3], "a tangent point latches 1e-3 first");
    check_parent_static_step("in the self-check").expect("a clean parent reads unlatched");
}
