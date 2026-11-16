# blend.fillet_custom_solver (FFI validator audit, 2026-10-14)

- **Validator vs shim.** `validate()` reads every op's input in the shim's order and
  counts, and its bounds match what upstream indexes: `crX[s1|s2 (+1)]` with
  `lSurf1Off`/`lSurf2Off` derived exactly as `LoadJacobian` does (offset surfaces share
  their base pointer, so selectors 1/3 are base A, 2 is B or A, 4 is the copy), `rF[ne..ne+2]`,
  Jacobian rows `ne..ne+2` and columns `s1, s1+1, s2, s2+1`; `LoadInitialValues` reads
  `rGuessT[s1_index (+1)]`, `rIntervals[s1_index (+1)]` and writes `rIntervals[ng (+1)]`
  (`SmExtentNd` has inline storage for 10 and allocates the rest). `CreateSurface` only
  reads `crFilletPoints[0..k)` after checking `k` against both rails' unique knot counts,
  and `N_CrvsMakeCompatibleAdjKnots` rescales only when the rails' intervals differ. The
  output buffer is retried at the size the shim reports. No gap found there.
- **ASan run.** SMLib, SM_API and the shims built with `-fsanitize=address` (a private
  build, not `bin/cpp`): 20,000 fuzzed `eval` inputs (all five ops, boundary counts and
  offsets, same / different rail intervals) were clean. One case in the 448-case corpus
  was not: `gen0137` (a valid `Create`, same-interval quadratic rails) reads freed memory
  in `CreateSkinnedSurface`'s skinning retry (upstream UB-99). The FFI guard cannot
  predict that the first skinning attempt fails without running NLib's skinning, so it
  still admits such inputs; `gen0137` passes against the release C++ (whose output there
  depends on the freed memory's contents), and its result should not be taken as proof
  of upstream behaviour.
