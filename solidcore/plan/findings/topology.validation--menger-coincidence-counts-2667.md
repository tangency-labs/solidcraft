# Menger coincidence work counts (#2667)

Addendum to [the baseline reproduction](topology.validation--menger-current-main-2667.md).
Instrumentation revision `e9f891dbd`, kernel base `513e6fcfc`, optimized `difftest`
profile, default kernel options, `profile` feature enabled. This records work,
not an optimization. The recipe and independent-copy protocol are unchanged.

## Repeatability and geometry

The [numeric record](menger-coincidence-counts-2667.json) contains two complete
fresh-process runs. Every topology count, validation result, coincident-pair count,
boolean work count and final volume equals the three uninstrumented baseline runs.
All 27 bodies pass Level2 and have zero coincident pairs. Final topology is
414 faces / 984 edges / 488 vertices, with volume `49999.99999999997`.

Every phase counter agrees exactly between the two runs. The nested Level2 and
standalone coincidence counters also agree for each cut on this recipe. Their
durations remain separate observations with different cache histories.

## Counts across all 27 cuts

The following totals apply independently to each diagnostic path; do not add the
root scope to its children (root counters include all children).

| Phase | Candidate pairs | Query invocations | Trim classifications |
|---|---:|---:|---:|
| vertex–vertex | 0 | 0 | 0 |
| edge–edge | 0 | 11,016 | 0 |
| face–face | 0 | 0 | 0 |
| vertex–edge | 22,032 | 0 | 0 |
| vertex–face | 22,228 | 200 | 200 |
| edge–face | 59,440 | 37,408 | 276 |
| **Root total** | **103,700** | **48,624** | **476** |

Candidate pairs are supplied by the existing spatial/membership selection, before
incidence filters. Edge–edge counts the supplied batch, which need not all be
visited when a first match terminates a search. Query invocations include empty
edge searches: all 11,016 edge–edge calls here receive no candidate edges. Thus
that number is not 11,016 curve comparisons. Classification counts are calls
after successful projection, not iterations internal to a classifier or solver.

The final cut has 10,896 candidates, 5,976 query invocations and no trim
classifications. Of those queries, 984 are empty edge searches and 4,992 are
edge–face checks. These counts identify edge–face projection as the next place to
inspect. They do not prove that it dominates elapsed time, or that these checks
can be skipped correctly. Removing empty calls alone would be a constant-factor
change, not an established fix for the reported slow construction.

Instrumented duration totals (seconds; include profiler overhead):

| Run | Booleans | Level2 validation | Standalone coincidence |
|---|---:|---:|---:|
| 1 | 0.523554802 | 3.404227176 | 1.380050126 |
| 2 | 0.524834458 | 3.420153431 | 1.379255593 |

These are not replacement baseline timings and are not performance thresholds.

## Validation

The 24 tests selected by the library `profile` filter pass, including the new
incidence-filter counter test and first-match early-return scope test. Clippy
with warnings denied and the wasm check pass with the feature enabled.

Pinned v1.0.1 native comparison with profiling enabled:

```text
PASS topology.brep_validation cases=112 passed=112 failed=0 deferred=0 max_rel_err=0e0
PARTIAL topology.assert_valid cases=965 passed=957 failed=0 deferred=0 max_rel_err=0e0 rejected=2 known_mismatch=4 parked=2
```

The eight non-passing classifications already exist in the unit's fixtures and
mismatch records. No mismatch classification, tolerance, unit status, geometry
formula or traversal order is changed here. This is not a claim that the partial
unit is green. The [broader validation addendum](topology.validation--menger-broad-validation-2667.md)
records the complete test run and full native sweep comparison with exact main.
