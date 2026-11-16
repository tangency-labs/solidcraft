# blend.fillet_executive_p2: opt-in generic radius setup error propagation (2026-10-26)

Units: `blend.fillet_executive_p2`, `blend.fillet_standard_solver_p1`;
UB-252 / #458 follow-up; PR #644.

Main already prevents reads of an uninitialised radius after a failed law evaluation
and reports the failure from `setup_offset_values`, `TemporaryRadiusChange::set_up`
and the first StepSolve setup. Issue #458 is closed by that work.

The generic constructor still deliberately ignores ordinary setup statuses, like
the pinned C++. This follow-up adds default-off
`Kernel::fixes.temporary_radius_setup_errors`, passed through additive
`TemporaryRadiusChange::new_with_fixes` and
`with_temporary_radius_change_with_fixes`. Existing call shapes retain their
ordinary-status behavior. With the switch enabled, the setup error reaches the
caller before the filleting body runs. Unported/preparation failures still
propagate under both policies; unconditional UB-252 protection remains in place.

The runner regression checks default body entry versus opt-in body exclusion on
an adapter setup failure and verifies that the original linear law and solver
list remain unchanged. The kernel regression evaluates a failed law with two real
offset surfaces and checks that neither offset distance changes.

Scope is the generic constructor and wrapper only. `DirectTemporaryRadiusChange`
retains main's documented status behavior and is available as an independent
follow-up. This PR adds no end-to-end variable-radius continuation, reference
modification, comparison omission or widened tolerance.

## Review validation (2026-10-27)

On main `7e6ef6679` plus this change, pinned Rust 1.98.0 / macOS arm64:
2,362 kernel tests, the native-feature constructor/body regression, plain and
native workspace Clippy with warnings denied, formatting, wasm32, and the
harness infrastructure tests pass. The rebase fixture includes main's new
`corners` and `edge_caches` fields.

The prescribed `--profile difftest` comparison passes both affected units:
executive p2 has 278 passes; standard solver p1 has 3,828 passes and 11 existing
rejected inputs, with zero mismatches or deferrals. No tolerance, reference input,
FFI guard, or known-mismatch entry changed.

A separate unoptimized Rust run differed on five standard-solver cases:
`rev_twist_n7p3_t1e-05_m0_c1_dr1e-06_g1_s1.0`,
`rev_twist_n7p3_t1e-05_m1_c1_dr1e-06_g1_s1.0`, `gen0040`, `gen0248`, and
`gen0295`. The same source, inputs and C++ reference pass in the prescribed
profile. These debug diagnostics are retained as profile-dependent observations,
not reclassified as known mismatches or hidden by a looser comparison.
