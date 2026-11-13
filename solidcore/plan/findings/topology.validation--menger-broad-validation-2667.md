# Menger instrumentation: broader validation (#2667)

Kernel base `513e6fcfcd049d41e9487599880479de4add63f2`; instrumentation
`e9f891dbd`; final review-only documentation/derived-volume revision `cadfe4923`.
No geometry, pruning, tolerance or mismatch-classification changes.

## Complete native sweep

The optimized profiler-enabled runner completed `--green --jobs 2` against the
pinned v1.0.1 reference:

```text
difftest: 280 unit(s), 254 passed, 24 partial, 2 failed
```

Across 591,949 cases: 589,761 passed, nine failed, 631 rejected, 1,402 known
mismatches and 146 parked. The failing units are `heal.gap` (seven cases) and
`primitive.primitive_creation_p2` (two). This is **not** a green full native sweep.

The exact-main comparison source is the `difftest-reports` artifact, ID
`13477299760`, from [CI run 42162728256](https://github.com/tangency-labs/solidcore/actions/runs/42162728256)
at that same base revision. Its ZIP SHA-256 was verified as
`ae2f4f1dd004170c774680a19c585a9226ea6c4ca0c7de8f8c7d32276347a22b`.

All 280 reports were compared as parsed, canonical JSON, not only by their
summary totals. Of these, 277 are identical. Two more differ only in checkout
prefixes of fixture paths in recorded inputs (`offset.offset_executive_p2` and
`blend.fillet_executive_p3`). Replacing those prefixes with the same placeholder
makes their complete reports identical; all input values and mismatch payloads
are otherwise unchanged. Machine-specific paths are not published here.

The remaining report, `sweep.topology_sweep`, differs only in `max_rel_err` for
`nm_test_rotation_y_30_stitch` and the corresponding unit aggregate:

| Observation | Maximum relative error | Outcome |
|---|---:|---|
| Exact-main CI artifact | `5.940235728088039e-16` | pass |
| Full instrumented sweep | `3.62089958464889e-14` | pass |
| Three isolated reruns, each | `2.5779867592970114e-15` | pass |

All use the unchanged `1e-9` relative criterion. The report does not retain raw
values for passing cases, so this comparison does not identify which side or
execution-context difference accounts for the rounding-level variation and does
not establish bit-identical floating-point output. Every other report field,
case outcome, comparison count and recorded mismatch payload agrees with main.
No tolerance was relaxed and no new failure was hidden or parked.

## Other checks

Workspace formatting and workspace/all-target Clippy with warnings denied pass.
Wasm checks pass with profiling enabled and with default features. The complete
optimized profile-enabled test run passes **5,086 tests**, with zero failures and
44 ignored: 3,571 library tests plus 1,515 integration tests. Four library tests
and 40 integration tests are ignored; the documentation-test target has no tests.

The final example exits successfully. Every geometry count, validity result,
coincident-pair count, boolean work count, coincidence phase counter and final
volume equals the archived profiled run, after excluding the three elapsed-time
fields. The named analytic volume constant introduced during review does not
change the expected value. The three source files on the validation machine
match the submitted files by SHA-256 after normalizing line endings.

The initial default debug build was stopped before test execution because its
generated integration executables consumed excessive disk space; those artifacts
were removed with the repository cleanup tool. The complete suite was restarted
with the repository's optimized `difftest` profile. The stopped build is not
counted as a passing check.
