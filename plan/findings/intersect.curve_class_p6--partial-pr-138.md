# intersect.curve_class_p6 (partial; PR #138)

- **Certified scope.** 4438 default cases: 3927 bit-identical kernel comparisons,
  6 expected FFI rejection checks, zero failures and 505 explicit deferrals; no absolute floors. The compared paths are
  non-splicing `InsertCurveInterval`, empty `Homogenize`, and the early
  `MergeInterval` exits. The native oracle calls the real four upstream members;
  geometry evaluations and interval/point-classification fields are compared along
  with statuses and created-entity output arrays. Successful kernel cases compare actual Rust and C++ operations. Explicitly
  marked rejection fixtures validate the oracle contract separately and are
  recorded as both-errored cases, not numerical kernel comparisons.
- **Dependency boundaries.** Nonempty homogenization and endpoint repair first need
  `GetPointClassByIndex` (p7). Insertion that splits or squeezes needs p2; its
  coincident-maybe decisions need p7. The last homogenization squeeze belongs to p4.
  `MergeInterval` stops at `GetPointObjZoneTol3d` (p2), before the remaining topology
  mutation. That continuation is not implemented or certified by this port.
  Existing generic `point_class` tolerance access also defers on `topology.edge`
  and `topology.face_p2`: these are adapter gaps in the opaque `ClassObject`
  interface, even though those topology units already have arena APIs. The new
  unit does not substitute stand-in topology results for these calls.
- **Fixture scope.** The native side builds real `SmLine`, `SmCircle`, `SmEllipse`
  and quadratic/constant `SmBSplineCurve` objects, and obtains topology from an
  actual box. Fixtures exercise all operations and flag combinations, end and
  interior subintervals, stored SSS points, adjacent short spans, degenerate
  curves, and scales 1e-15 through 1e9. They do not certify 2D classification or
  the deferred topology mutation. Constructor status/null checks precede topology
  traversal; objects are scoped and destroyed after the classifications.
- **Safety and recursion.** FFI validates complete bounded records, partitions,
  enum values, flags and operation indices. Invalid geometry-as-topology merge
  inputs are rejected; safe vertices-only paths remain accepted. A 27440-process
  regression matrix checks all seven classification kinds at each interval point,
  operations, true geometric kinds and flags with a five-second per-child timeout, separately from differential pass counts. Tiny
  and large constructor paths and malformed records have dedicated native tests.
  The native FFI suite is run locally with the existing CI workflow unchanged.
  Rust insertion replaces tail recursion with the same restart order, bounded by
  the initial interval count plus one; homogenization refuses a squeeze that makes
  no progress.
- **Port audit follow-up.** Exhaustive native merge probes exposed unresolved
  UNKNOWN starts (EDGE/VERTEX interior or failed FACE projection), recorded as
  UB-122 in `plan/upstream-issues.md`. The shim preflights the actual projection
  sequence and rejects only unresolved-start full merges, retaining constant-curve
  and vertices-only returns and successful FACE projections. Rejections are fixture
  errors, never parity passes. Another 96 isolated probes cover partition sizes
  1/2/3/32, saved SSS points and boundary indices. `FixEndPointClass` now receives the
  caller's point on both sides; a native regression checks all eight box corners
  (exactly two promote the chosen edge to a vertex) and a distant control. Invalid
  Rust boundary indices return `InvalidInput` before dependency dispatch.
- **Coverage and ownership.** Added 3645 receiver/incoming classification combinations
  over UNKNOWN/CURVE/NOTHING, preserving gaps, parameters and saved points. Generated
  cases now exercise all four operations. Existing p1 types and p3 helpers are reused;
  p2/p4/p7 implementations remain owned by their open PRs, without duplicate co-ports.
  Perturbation detects all 3933 p6 outputs (3927 kernel results and 6 expected
  rejections) and 2064 p3 outputs. No absolute floors or
  tolerance changes were added. Nonempty endpoint repair remains uncertified in Rust
  until p7 lands; its caller-point regression is a native contract test.

- **Maintainer review (PR #138).** `MergeInterval` with fillets now requires
  equally sized partner classifications in both `validate` and the raw C++ shim,
  preventing `sm_SetDoFilletingGeometry` from indexing an absent partner interval
  (UB-136). Six `rejected_fillets_*` fixtures cover empty, short and long partners,
  both full and vertices-only merges, and VERTEX/EDGE/VERTEX, FACE and REGION
  classifications. Another 660 subprocesses check wrapper rejection and raw-shim
  rejection independently at first/last indices of 1/2/3/32 intervals across all
  five geometry kinds. Positive controls invoke native merging on equal-sized
  pairs and on unequal pairs without fillets.
  Both final homogenization squeezes use `ignore_status`, retaining propagation
  of `Unported` and the count-decrease guard (HANG-14). Boundary locations reuse
  p3's `classify_crv_loc_by_index`; its single stub owns the `UNPORTED` tag, so the
  misplaced p6 tag and duplicate `boundary_location` stub are removed. STATUS
  describes the unit as partial without the obsolete claim that the PR is draft.
