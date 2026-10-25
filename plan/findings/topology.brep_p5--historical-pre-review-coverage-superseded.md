# topology.brep_p5 — historical pre-review coverage (superseded)

Historical evidence only (`95967da6` / `89a20913`); see the current review
validation section below. These results do not certify the review fixes.

- **Independent partial scope (PR #194).** Selected from official main `433e1b2c`:
  no unclaimed todo unit had all required dependencies green/partial, and this
  was the lowest-layer unclaimed unit (47). No open dependency PR is bundled.
  The twelve editing entry points have explicit dependency barriers; continuations
  after those barriers are **unimplemented and uncertified**, rather than a complete
  port of all 2,193 estimated upstream lines.
- **Numerical-stage oracle.** `coincident_vertex_target` factors the old-tolerance
  position/tolerance calculation from `CombineCoincidentVertices`. Its native
  oracle calls the real merge on shell vertices/wire stars and reads the surviving
  point and tolerance. Rust computes only that stage; these comparisons do **not**
  certify deletion, vertex-use transfer, shell merging or interval repair. Cases
  cover different incident-edge counts, both argument orders, zero/coincident
  positions, strict tolerance boundaries, negative raw tolerance conversion and
  scales `1e-9..1e9`. The computed temporary-edge length threshold is source-traced;
  its consumption in the common-edge squeeze decision remains uncertified.
  Native `SmCurve::Length` writes its output only on successful returns, so an
  ignored failed integration retains this caller's initialized zero.
- **409 numerical-stage comparisons** and **269 whole-operation comparisons**
  are reported separately. The latter cover editing-disabled refusals, null-face
  insertion refusal, empty region selection, empty face removal and the identity
  transformation exit. Status and initialized/preserved output state are compared
  alongside the shared semantic topology/geometry snapshot. Empty face removal
  also advances/releases a context traversal mark; the final review fix ports
  that lifetime and adds native counter/lock comparisons, including an existing
  outer mark lock. These operations do
  not certify successful topology mutation. In particular, `SqueezeFace` resets
  all output arrays *before* its editing check, whereas `MakeEdgeInFace` preserves
  its outputs on the editing-disabled/null-face checks. Identity transformation
  uses native placement tolerance and native vector equality, and exits before
  tolerance scaling even when that flag is true.
- **Actual blockers.** Owning-list traversal, infinite-region lookup and the
  owning B-rep's editing member are `topology.brep_p1`; `SmBrep::Notify` is
  `topology.brep_p2`; common-edge squeezing is `topology.brep_p3`;
  `SmBrep::StitchFaces` is `topology.brep_p4` (not `heal.stitch_p1`);
  `FindInfiniteRegion` is `topology.brep_p9`; face sector classification and
  boundary collection are `topology.face_p4`. The void-setting flag remains a
  raw code at the deferred boundary because part 1's active PR already owns its
  common enum; that dependency type is not duplicated. Missing edit state is not replaced
  with a hard-coded true value or arena allocation order.
- **Back-edges.** The API transformation target now carries the actual mutable
  arena. Healer parts 1/3, boolean post-processing and proto-topology parts 1/3
  call part 5. The opaque glue entry stops at the real notification dependency
  before dereferencing any handle; it constructs no substitute B-rep. The
  handle-only proto-edge insertion path lacks owning B-rep editing metadata and
  now defers on that part 1 record gap. These stores still need their actual
  mutable-owner/notification adapters when mutation continuations are implemented.
  Downstream mutation/change-tracking continuations are not certified by the
  remaining deferrals; no dependency owner is recertified or promoted here.
- **Native fixture boundaries.** The shim constructs actual `SmBrep`, wire stars,
  the wire network from upstream `my_test_topology`, a plane face and a real
  non-normalized `SmLine`. Null/aliased merge vertices and insertion with missing
  required geometry are refused before native UB. Zero, tiny and parallel
  placement axes remain accepted: `SetCanonical` initializes its members before
  an error and has defined partial/fallback state. The context's default model
  size is restored before each fixture so a scaling case cannot contaminate the
  next fixture or a buffer retry. No extra absolute floors are used.
- **Remaining handoffs.** Nonidentity transformations, NURB conversion/reflection,
  tolerance scaling, general vertex/edge/face mutations, region nesting and ray
  selection, stitching and its late-failure outputs, attributes/caches/callbacks
  and successful downstream change tracking remain unimplemented or uncertified.
  The call-graph's two `SmPendingTopologyDelete` entries are an internal reader
  RAII helper, not an independently exposed editing API: owned Rust arguments
  drop on failure, but borrowed-attribute detachment in `MakeTopologyFromData`
  remains for that reader's owner. The high-level trimmed-surface/circle/API
  intersection tests in the unit's call-graph list are not certified end to end.

- **Validated subset and environment.** Functional source `89a20913` on runner 2,
  Ubuntu 26.04 x86_64, Rust/rustdoc 1.98.0, GCC 15.2.0; one Cargo job under nice,
  standard difftest profile and C++ `-O2 -ffp-contract=off -fno-fast-math`.
  All 490 allowed upstream source/header files match `c9c5979`; existing reference
  libraries are reused. Rustdoc 1.98 is selected through `RUSTDOC` because the
  runner's system rustdoc is older; there are no repository platform/build changes.
  The first native executable rebuild exhausted the runner's 8 GiB RAM (kernel OOM-killed
  rustc). Retrying with a temporary 4 GiB swap file preserves the standard build
  profile; no compiler or numerical settings change. The swap file was disabled and
  removed after compilation.
  Formatting, strict plain/native-enabled workspace Clippy, all 1,282 kernel
  tests, WASM checking and two native safe-FFI tests pass. The eight affected
  focused runs have no failures. The complete `--green --no-report` gate passes
  across all 200 green/partial units; `--status-of topology.brep_p5` reports
  `partial`. Exact summaries:

  ```text
  PARTIAL topology.brep_p5 cases=724 passed=678 failed=0 deferred=46 max_rel_err=0e0 blocked_by=topology.brep_p1(10),topology.brep_p2(32),topology.brep_p4(2),topology.brep_p9(2)
  SENSITIVE topology.brep_p5 perturbed=678 detected=678 unperturbed=0 deferred=46
  PARTIAL topology.brep_p5 cases=4308 passed=4136 failed=0 deferred=172 max_rel_err=0e0 (--seeds 4096 --no-report)
  difftest: 200 unit(s), 108 passed, 92 partial, 0 failed
  --status-of topology.brep_p5: partial
  ```

  There are 212 unique fixtures and a minimum of 512 generated cases. The report's
  `seeds` field is the requested CLI value (16 by default), while the per-case source
  entries enumerate the actual generated seeds. Six existing affected reports
  regenerate byte-identical; proto-topology part 1 changes only its ten part 5
  transform deferrals to the actual part 2 notification blocker. No other unit's
  status changes. Port-pr-review checked the independent diff/ancestry, native
  defaults/operation order, output lifecycle and empty-list context mark state,
  real fixture paths/guards,
  input/collection safety, ownership and revision-specific coverage. No confirmed
  defect remains in the exercised subset; the handoffs above remain uncertified.
