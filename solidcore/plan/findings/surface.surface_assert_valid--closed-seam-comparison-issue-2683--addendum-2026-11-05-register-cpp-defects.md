# surface.surface_assert_valid: register the measured WR-752 cases (2026-11-05)

Updates: plan/findings/surface.surface_assert_valid--closed-seam-comparison-issue-2683.md

Follow-up PR #2786 to the merged correction in #2784, requested by @jyviko in [this comment](https://github.com/tangency-labs/solidcore/pull/2784#issuecomment-6654591429), using #2711's per-case evidence rule. The two new `cpp_defect = "WR-752"` entries cite `plan/upstream-issues/WR-752--point-grid-misses-last-turn-at-closed-seam.md`, whose confirmed record already names `gen0340` and `gen1316` and measures their exact vectors and angles. The merged finding and upstream record are unchanged.

Main `b1f6d207baf2841b691261413967b2d96d37b37a`, seeds 16, before the entries: 2,902 cases, 2,566 passes, 4 failures, 329 rejections and 3 parks. With the entries: the same 2,902 cases and 2,566 passes, 2 failures, 329 rejections and 5 parks. Only `gen0340` and `gen1316` change from `fail` to `parked`; every other case report is identical in fresh local runs before and after registration. They are reported as confirmed reference defects, not counted as passes. The remaining failures, `gen0812` and `gen1016`, are the separate WR-737 work in #2775.

Running `--unit surface.surface_assert_valid --run-parked` executes all five listed defects. Both new cases still compare C++ `values.out.len = 3` against Rust `157`, and are classified `cpp_defect` with ID WR-752; the three existing cases remain WR-737 defects. This mode reports five C++ defects and zero ordinary failures, but its command exits nonzero because the existing value-share gate requires at least one passing value-bearing case and a parked-only run has none. No value-share waiver is added.

The registry/evidence tests pass (`cargo test --profile difftest -p difftest --lib`: 116 passed, 2 existing ignored tests). The normal unit run remains nonzero solely for the two WR-737 failures above. Kernel code, numerical thresholds, FFI guards, inputs and generators are unchanged.
