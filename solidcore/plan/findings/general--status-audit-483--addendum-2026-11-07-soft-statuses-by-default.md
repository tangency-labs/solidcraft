Updates: plan/findings/general--status-audit-483.md

## The fork's four soft failures are the default; their flags are retired (#2009)

`fillet_corner_statuses`, `fillet_edge_curve_domain_statuses`, `offset_tangent_gap_statuses`
and `ssi_alignment_statuses` are no longer read: the fork's choice (tangency-labs/smlib
66b02de, 4803d6d, e7ebc8d; see the 2026-10-31 and 2026-11-01 addenda) is the only path.
The fields stay until SolidCraft's set drops them.

- `CheckAndFixDegeneracy` keeps a vertex's UV point when its projection fails (it wrote
  `SM_UNDEF_DOUBLE`); `sm_TestG1Continuity` finds no tangency without an end tangent (the
  old path found none either: the unset tangents are shorter than `SM_EFF_ZERO`).
- `sm_CheckEdgeCurveDomains` returns no trimmed curve when the trim fails (it returned the
  untrimmed copy).
- `HaveTangentGaps` skips a sample it cannot evaluate (it measured the unset point).
- `ALIGN_DIR_WITH_PREVIOUS_POINT` aligns nothing without an angle; the old path gave the
  same result (an angle read as 0 is never near pi), so no test can fail without it.

Four tests on the default kernel fail at the test commit ddf2cf6a1 and pass after it:
`degeneracy_fix_ignores_a_failed_uv_projection` (UV point (1.23456e-61, 1.23456e-61)
instead of (4, 5)), `audit_483_edge_domain_is_no_trimmed_curve_after_a_failed_trim`
(`Some(copy)`), `audit_483_tangent_gaps_preserve_errors_and_no_solution` (6 calls instead
of 3) and `tangent_gaps_skip_a_sample_that_cannot_be_evaluated` (a gap).

Goldens at 26bc871b7 (base 7a593124c): `cargo run --profile goldens -p goldens -- --all
--goldens`, 283 units, 283 passed, no case changed.
