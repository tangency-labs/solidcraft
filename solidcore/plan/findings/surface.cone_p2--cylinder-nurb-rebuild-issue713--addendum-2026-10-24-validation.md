# surface.cone_p2: final validation of #713 after merging main (2026-10-24)

Updates: plan/findings/surface.cone_p2--cylinder-nurb-rebuild-issue713.md

Validated source commit `14660414b4436b899ef6e18cb4ed27c266c39761`, which includes
upstream main `c3e81f085c0396d0d5a7d1fd82687852b4aed2ab`, for draft PR #724:

- Formatting, strict workspace Clippy (default features and C++), and the
  `wasm32-unknown-unknown` check pass.
- 2,099 kernel unit tests, the ordinary integration test, four optimized primitive
  integration tests (one pre-existing ignored test), and the three cone-p2 runner
  tests pass.
- The complete `--green --jobs 4` run finishes with 259 units: 211 passed,
  48 partial, and zero failed, across 553,536 cases. This uses the existing
  deferred/known-mismatch handling and excludes the blocked `surface.cone_p2` unit.
- The separate raw `surface.cone_p2` run still has 1,594 cases: 1,542 passed,
  52 failed, zero deferred, `max_rel_err=1e0`. Its report is identical to the
  pre-merge fixed implementation's report. All 52 differences remain documented
  in the earlier #713 entry; the unit remains blocked against the pinned C++.

No generated reports are committed and no new comparison exemptions are added.
