Updates: WR-616

#### WR-616 (update, 2026-11-02). Thousandth-grid setback scale search (`fillet_blend_corner`)

Issue #2344(a), SMLib commit `20b1c01`, replaces the margin/secant prediction and
multiplicative steps ported by #2369. It runs under the same existing opt-in
`Kernel::fixes.fillet_blend_corner`; default corner behavior stays pinned.

`blend_setback::certified_scale` tests the actual certified fill at indices 1000
and 2000, then bisects integer indices until the failed and certifying ends are
adjacent. Every setback is computed as `s_i * (i / 1000.0)` before being held at
its edge's room. The returned scale was itself tested. This is **the smallest
certifying scale on the grid whose neighbour below fails; monotonicity checked,
not proved**. Certification selects a least-energy QP solution and uses a fixed
subdivision depth, and room limits can stop individual edges growing; none of
these establishes global monotonicity.

`CertifiedScale::attempts` counts full fill attempts (at most 12); `solves` counts
the QPs they solve. Focused tests cover the grown scale 1.056, held-edge 1.046,
asked setback 1.110, original-scale certification, and refusal when every edge is
held at its original setback. A separate explicit `setback_monotone_grid` test
checks seventeen corner descriptions (axis permutations, concavity, unequal and
equal radii, requested setbacks and held rooms), at 1, the three thousandths on
either side of the result, +0.020, +0.100, 1.5 and 2. These are this contribution's
matrix, not a claim to reproduce all seventeen SMLib capi fixtures.

This slice preserves degree order, stored sector rows, the QP algorithm and the
public probe API. The streaming WIP `3843ddb`, degree-nine-first evaluation,
row/peak-row/active-set-step counters and their full result-equivalence gate
remain follow-ups. No unit is promoted by this contribution.

Historical evidence at `274e0a4a6`: the native executive comparison passed on twelve independent inputs:
six built bodies and 144 vertices. Status/refusal, comments, named fits, topology
counts, volumes and every vertex coordinate agree to 1e-9 relative without an
absolute floor. The reference overlays the complete changed SMLib source set at
20b1c01 on the existing 5e8c9c1 libraries, recompiling blend/fillet consumers of
the changed class layouts. The reproducible driver, build script and strict
comparator live under `harness/cpp/tests/`; commands and limitations are in
`docs/setback-scale-search-validation.md`. This does not certify the later main merges:
the expanded thirteen-case comparison has strict near-zero coordinate mismatches,
and four legacy flag-off snapshots fail. Both are recorded in the
`plan/findings/blend.fillet_corner--2344-main-refresh-*` findings. The existing
blend-corner flag is temporary until its complete construction is accepted; it
must then become the default with the old path and flag deleted.

Latest resolution: main #2491/#2495 excludes the explicitly solidcore-native
exact-spine construction from these fork-parity tests, with original snapshot
assertions active and unchanged. After merging main `84a419512`, this branch's
aligned fork-profile executive passes all thirteen inputs, seven bodies and
167 vertices at strict relative 1e-9 without a floor. The seven regular setback
tests pass. This supersedes those historical profile mismatches; it does not
revert the native construction or loosen the comparator.
