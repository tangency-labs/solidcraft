# geom.local_solve_nd: revalidation after the exact nodal fix (2026-11-07)

Updates: `plan/findings/geom.local_solve_nd--goldens-default-2026-11-06.md`.

Issue / PR: #2782 / #2791. The earlier record keeps the November 6 measurements and
their then-failing #2508 assertion as historical evidence. #3022 fixes #2508 and merged
at `6a0862fde`. The N-D branch is now rebased linearly onto `f4f70127a`, which also
contains #2998's law-corner definition correction.

## Integration

The rebase preserves #3022's direct Bernstein spine construction and all its exact
nodal definition, record, reparameterisation and corruption-refusal tests. The traced
round's independent radius/area and clean Level-2 checks from this PR are retained.
The exact-class accuracy assertion remains at 1e-9, with no ignore or tolerance change.

Main's new known-failure entries are preserved. The only added entry is still
`blend.fillet_executive_p3/stubs4-open_shell-e5-r0.5` (#2956), whose nine pre-existing
Level-2 gaps are documented in the original record. All 40 known failures execute;
none has a blessed golden line.

## Golden evidence

A fresh 32-unit measurement against main `6a0862fde` gives the same 1,332 cases beyond
the unchanged comparison threshold. All 1,398 blessed case records have exactly the
same old and new JSON values as the November 6 measurement; all 1,398 measured current
records also equal their blessed values. There are no new case IDs or unexplained
changes in that set. The original per-case ledger, geometric checks and paired
diagnostics renders therefore remain the evidence for those changes.

The executive golden was resolved case by case during rebase, preserving every new
main case and known-failure removal. Its serialization was normalized through the
official `goldens::golden::read` / `write` functions without changing any value. Its
final diff is two changed case lines and the #2956 line removed.

The full goldens check on `6a0862fde` passes all 283 units. The final revalidation on
`f4f70127a` is recorded below. Reproduction observers and rendering commands live in
the PR description, rather than adding executable measurement tooling to the record.

## Final validation

On `f4f70127a` plus this PR, with `CARGO_BUILD_JOBS=2`, `RUSTC_WRAPPER=sccache`
and commands under `nice -n 10`:

- `bin/goldens/goldens --all --goldens --jobs 4`: **283 passed, 0 failed**;
  40 known failures executed, no added, removed or broken case.
- `cargo fmt --all --check`: pass.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: pass.
- `cargo check --offline -p solidcore --target wasm32-unknown-unknown`: pass.
- `python3 -I scripts/ci/check_panic_sites.py --base solidcore/main`: pass; no new site.
- `scripts/ci/check-frozen-archives.sh solidcore/main` and
  `python3 -I scripts/ci/check_record_ids.py`: pass.
- `cargo test --offline --workspace --profile goldens --doc`: **2 passed, 0 failed**
  across 12 crate targets.
- The workspace run includes **15/15 direct N-D solver tests** and **4/4 tests** in
  `wr669_exact_nodal_quartic_round` passing, including the formerly failing #2508
  accuracy assertion.
- `cargo-nextest nextest run --offline --workspace --cargo-profile goldens
  --no-fail-fast --test-threads 4` (CI-pinned 0.9.150): **6,566 passed, 0 failed**,
  99 existing skips. `SOLIDCORE_REQUIRE_UPSTREAM=1`.

## Kernel-rig deadline reproduction

The additional full CI rig replay passes `union_all_replays_captured_cases`, but
`kernel_cases` stops at `fillets/cov-fillet-section-curvature-boss-root-r2` after the
unchanged 180 s deadline. Its table grades 368 cases: 285 pass, 69 unsupported,
13 known failing and one deadline failure; 711 later cases are not run. The same
symptom is already reported on main in #2989. The isolated PR replay also reaches
180 s. An isolated unmodified-main control at `f4f70127a` passes in 175.49 s.
After compilations and the goldens finish, the isolated PR replay passes in 143.57 s.
Both passing runs grade its relative volume error as 1.4e-10. These observations
reproduce the intermittent deadline reported in #2989; they do not establish a
comparative operation cost or its geometric cause.

The timeout, tolerances, fixtures and known-failure classification are unchanged.
The failed full run is reported explicitly, with its 711 unrun cases, rather than
waived or counted as a pass. The PR description records subsequent full-corpus
validation results.
