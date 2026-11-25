# UB-170: explicit native-guard reporting migration

PR #1487, integrated main 82e85f985ca5a68b89ee602976eab33541d8d94b.

The 17 gen cases listed in blend.hcr--ub170-safe-continuation-guard-contract.md never execute the guarded native HCR operation. Their baseline values are the empty-record -2 refusal sentinel, not operation results. Reporting them as rejected corrects the classification. The case definitions, native guard and all numerical tolerances are unchanged.

The repository's case-specific ratchet entries identify exactly these 17 reporting transitions. Their native-refusal contract remains asserted in guarded_ub170_cases_complete_independently_in_rust, alongside independent successful Rust execution with expected classification record lengths and finite values. native_guard_is_rejected_without_hiding_operation_results also preserves real -2 operation records. This migrates sentinel accounting without deleting a geometric comparison, suppressing a Rust mismatch, or waiving a whole unit. Remove these transition entries once the merged base reports use rejected.

Integrated native results: blend.hcr 476 cases, 459 passed, 17 rejected, zero failed/deferred, max_rel_err 0; sweep.topology_sweep 703 cases, 693 passed, 2 rejected, 8 existing ND-491 parks, zero failed/deferred, max_rel_err 3.1974423109204407e-15. Full checks and exact-base validation are tracked in the PR. The private bottle result remains invalid after the diagnostic native correction; this scoped fix does not claim to repair it.

Final validation at 4064a00899: formatting, C++-enabled workspace Clippy, both UB-170 runner regressions, all kernel tests in CI's optimized difftest profile, and wasm passed. Full native sweep: 280 units passed, zero partial/failed. Full comparison against the provenance-validated exact-base 82e85 artifact: 280 reports, zero regressed units, with the 17 explicitly recorded reporting-migration entries. The earlier unoptimized test run was interrupted and is not counted as a pass.
