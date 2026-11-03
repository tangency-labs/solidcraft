# Streamed setback rows: native preparation for #2344(b)

This is a **preparatory validation contribution**, separate from the implemented
scale-search slice in #2382, whose latest reference-profile resolution and
verification are recorded in `setback-scale-search-validation.md`. Rust still
materializes its sector matrix.
No Rust row construction, QP arithmetic, degree order, probe API or flag changes
are made here.

The [#2366 boundary reply](https://github.com/tangency-labs/solidcore/pull/2382#issuecomment-6595523415)
leaves its curved module, stored rows and real-round growth with that PR. This
preparation freezes no Rust API and reserves no production streaming scope.
Any future shared provider must preserve slot/tie order, working-set exclusion
and count gates while supporting the curved polynomials and their degree order.

The allowed SMLib source at `3843ddb53e4a1a80d12691775f0b7a499a73d95f`
is compared with the stored-row reference at `20b1c01`. This WIP combines streamed
rows with degree-nine-first scale evaluation and removes the old probe API.
Passing this matrix does not certify all its behavior or authorize copying those
other changes into a row-only port.

## Gates and evidence

- Historical evidence at `274e0a4a6`: the twelve-case executive comparison passed
  against both stored-row C++ and
  the merged Rust scale branch: six bodies, 144 vertices. Status/refusal, exact
  comments, named fits, topology counts, volumes and every coordinate agree
  within 1e-9 relative, without an absolute floor.
- The six-case scale matrix preserves every scale and attempt count. Exact native
  WIP QP/row/peak/step counts are checked by `blend_setback_streamed_counts.cpp`.
  These are counts for the **combined native WIP**, not expected Rust row-only
  counts and not timings.
- `blend_setback_streamed_row_order.cpp` exercises the real private native row
  provider: stable patch/piece/coefficient/direction/pair slots, skipped slots,
  exact equal-slack ordering, exclusion of working-set rows, cheap-bound skips
  and agreement with an independently materialized dense QP.
- A matrix with 2050 equal violated rows checks the 2000-row working-set block.
  The small dense-equivalence and block cases each take three dual active-set
  steps, including the unconstrained solve.

| Case (same order as the scale gate) | Scale | Attempts | QPs | Rows built | Peak held | QP steps |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| radii 2,3,4, rule | 1.056 | 12 | 22 | 3594502 | 10000 | 6537 |
| radii 1,1.5,2, rule | 1.056 | 12 | 26 | 4569089 | 10000 | 8089 |
| upright held at 4.5 | 1.046 | 12 | 21 | 4542244 | 10679 | 8194 |
| asked setback 4.25 | 1.110 | 12 | 30 | 3257414 | 11422 | 7150 |
| radii 5, setback 8 | 1 | 1 | 3 | 7044 | 1950 | 22 |
| every edge held at its rule | 0, refused | 2 | 2 | 33152 | 3931 | 1324 |

## Reproduction

First build the stored-row executive overlay as described in
[setback-scale-search-validation.md](setback-scale-search-validation.md).
Fetch only `source/SMLib/inc/SmBlendSetback.h` and
`source/SMLib/src/SmBlendSetback.cpp` at `3843ddb` into a separate ignored
overlay directory. Do not substitute `5be7489`: that earlier work-counter commit
predates the scale/streaming changes.

```sh
bash harness/cpp/tests/build_setback_streamed.sh \
  bin/reference-2344 bin/reference-2344-stream \
  /home/karlvonrichter/github/smlib-fork /home/karlvonrichter/github/fork-cpp/lib
nice -n 10 bash harness/cpp/tests/run_setback_executive.sh \
  bin/reference-2344-stream/executive > /tmp/streamed-executive.log
python3 harness/cpp/tests/compare_setback_executive.py \
  /tmp/native-executive.log /tmp/streamed-executive.log --native-left
python3 harness/cpp/tests/compare_setback_executive.py \
  /tmp/rust-executive.log /tmp/streamed-executive.log
```

The build recompiles callers allocating and deleting `Fill` too: its WIP counters
increase its size, including the size used by sized delete. It reuses the other compatible objects and
existing libraries; the shared pinned C++ build remains unchanged. Every C++
command is under nice, and the builder compiles serially.

## Remaining implementation and coverage

The twelve-case result above is historical evidence at `274e0a4a6`. The shared
executive runner/comparator now requires thirteen cases, adding #2369's unequal
(2, 2, 3) box corner. Rebuilt native stored/streamed drivers pass the expanded
matrix: thirteen cases, seven bodies, 167 vertices, strict relative 1e-9 with
no absolute floor. Current merged Rust does not pass the same coordinate gate;
see `plan/findings/blend.fillet_corner--2344-main-refresh-relative-zero-parity.md`.
Native-to-native parity does not establish Rust parity.

Both dependencies (#2369 and #2274) are now merged. The resumed scale slice
preserves #2419's intentional exact-spine cylinder construction and diagnoses
its difference from the old native frame; see
`plan/findings/blend.fillet_corner--2344-exact-spine-reference-boundary.md`.
That diagnostic does not waive the normal Rust profile's strict comparison.

The production Rust streamed row provider, affine-polynomial storage, working-set
integration and per-kernel work counters remain unimplemented and available for
a separate claim. Preserve the existing degree order unless separately changing
and gating it; preserve the logical slot ordering even where a row is skipped.
Retain the existing opt-in behavior.

The pocket-rim fixture, the original full monotonicity inventory and broader
curved/degenerate/scaled streamed-fill coverage remain outstanding. This matrix
is useful native evidence, not a claim that SMLib's WIP or a Rust streaming port
is fully gated. No unit is promoted.
