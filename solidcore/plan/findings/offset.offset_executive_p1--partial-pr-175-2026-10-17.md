# offset.offset_executive_p1 (partial, PR #175, 2026-10-17)

The implementation, native adapter, FFI wrapper, fixtures and differential runner
are included directly in the PR rather than depending on a personal WIP branch.
Settings, correspondence, synchronization and extended-surface caching are
implemented; higher construction paths stop at explicit executive adapters or
actual unavailable callees, with owner-tagged `Unported` errors and documented
continuations.

- **Current batch #269 integration in PR #201 (main `fb58ea76`).** The default
  report now records 693 cases: 688 compared passes, four deferred, one native
  analytic-line constructor rejection, zero failures. Sensitivity detects 688/688;
  numerical tolerance and the maximum error remain unchanged. Main contributes
  twelve formerly deferred synchronization cases; these compare the fixture's
  native editing-disabled no-split behavior, not completed split construction.
  This integration adds one further comparison: `probe_remove_empty` calls the
  real `RemoveFaces`, whose ordinary editing-disabled error is ignored by native
  `RemoveExcessFaces` (`SmOffsetExecutive.cpp:2099–2102`). No inner guard or mark
  effect is bypassed, and missing implementations still propagate.
  The remaining first stops are the executive's VertexOffset arena/geometry
  adapter, `CalculateBoundingBox`, B-rep `ShrinkGeometry`, and the legacy
  normalized-evaluation adapter. Generic offset-geometry creators and stitching
  are now landed; their executive adapters must not be described as absent
  implementations of those dependencies. Earlier aggregates below are historical.

- **Extension input contract corrected after review.** The first investigation
  incorrectly treated signed geometry-creator distances as reachable executive
  inputs. `OffsetBrep` (`SmOffsetExecutive.cpp:114-120`) and `ShellBrep`
  (`:206-223`) use `fabs(dOffsetDistance)` for the creator and carry the sign
  through `SetInset`; the local-operation caller (`SmBrep.cpp:12004-12008`)
  uses half a bounding-sphere radius. The executive's internal extension
  magnitude is therefore nonnegative. The FFI and native adapter reject
  negative extension magnitudes before entering C++, with dedicated guard tests.
  Signed zero and positive tiny/large magnitudes remain covered. Signed
  constructor/getter probes and inset flags remain valid separate coverage;
  negative public offset distances are not rejected. HANG-16 records the
  underlying surface routine's direct negative-distance hazard, not a blocker
  reachable through the real executive entry points.
- **Equal-minimum relation order follows the established convention (ND-2).**
  `SmRelation::GetSeconds` iterates pointer-hashed unordered membership, and
  `sm_SortEdgeuses` swaps only for strictly greater minima. Following this
  file's existing `geom.tol` convention, both comparison snapshots sort only
  contiguous groups with exactly equal measured minimum parameters by traversal
  id. The original two tie regressions remain. Distinct-minimum ordering,
  membership, multiplicity, null padding, geometry, statuses and traversal
  records are unchanged; no tolerance is relaxed. Regression tests verify that
  normalization does not hide those differences. The deterministic kernel's
  actual sorting algorithm is unchanged.
- **Earlier aggregate validation after the non-blocking follow-ups.** The corrected
  runner completes without an isolated-case selector: 693 cases (93 fixtures and
  600 fixed-seed generated), 675 passed, 0 failed, 17 deferred and one explicit
  analytic-line constructor rejection. Every pass compares numerical values.
  Maximum relative error is `2.9158035700250084e-14`, within unchanged `1e-9`
  relative tolerance with no absolute floors. Sensitivity detects all 675
  perturbations. The standard report is
  `reports/difftest/offset.offset_executive_p1.json`; the obsolete Markdown
  investigation is removed. These results supersede the initial 691 / 633 / 17 /
  41 aggregate. Rejected inputs and deferred paths are not passes. Final validation
  uses pinned upstream `c9c5979`, GNU g++ 12.2.0 and Rust 1.98.0 on Linux ARM64
  in local Docker, not the retired network builder.
- **Original dependency boundaries (superseded above).** Twelve cases needed
  `topology.brep_p9::SmBrep::MakeVertexSplitEdge`; one each needs
  `heal.stitch_p2::SmStitch::StitchIntoSolid`,
  `offset.offset_geometry_creation::SmOffsetGeometryCreation::VertexOffset`,
  `topology.brep_p4::SmBrep::ShrinkGeometry`,
  `topology.brep_p5::SmBrep::RemoveFaces`, and
  `topology.edgeuse_p3::SmEdgeuse::NormalizedEvaluate`. Construction continuations
  behind these actual calls remain documented rather than reported as completed.
  Other unavailable construction branches have owner-tagged adapters but are not
  certified by empty/no-op cases. Review corrections also cover custom tangency
  snapshots, orientation-sensitive endpoint notifications, cache identity, strict
  length filtering, gap order, actual topology-solver dispatch and both retained
  equal-minimum regressions.
- **Earlier broader checks and limitation.** Formatting and C++-enabled workspace Clippy
  (all targets, warnings denied) passed. All 1299 kernel tests, 20 runner tests and
  14 FFI tests pass, including execution of the direct native negative-distance
  guard test; the wasm check passes. A full `--green --no-report` run selected
  198 units but hit its 1200-second limit after 107 PASS and 67 PARTIAL summaries,
  with no reported failures. It last completed `query.edge` and did not reach
  this unit at position 196. The standalone offset run above passed the partial
  gate; no completed full-suite baseline pass is claimed.
- **Non-blocking review follow-ups (2026-10-18).** `RemoveExcessFaces` now tags the
  arena-order edge walk with `UNPORTED(topology.brep_p1): SmBrep::GetEdges` and
  explicitly does not certify linked-list/deletion order; nonempty paths still
  stop at `NormalizedEvaluate` before removal. UV conversion and bounding-box
  calls preserve the ordinary statuses upstream ignores, without hiding
  `Unported`; tests check conversion side effects and the initialized empty
  extent on ordinary failure. The strut vertex's missing-mate lookup now yields
  an empty relation list, preserving primary-map precedence, rather than a null
  error. Five kernel regressions cover these changes.
- **Rejected-fixture coverage repaired without changing measured geometry.** The
  previous adapter used `SmBSplineCurve::CreateLineSegment` to close each measured
  loop, but that constructor rejects coincident endpoints. Only for exactly
  coincident support endpoints, a real nonzero closed cubic B-spline now closes
  the loop on both engines. The measured curve/interval, actual `MakeWireEdge`
  calls, measured traversal and normalization policy are unchanged. Both original
  zero-trim cases and the collapsed quadratic case now perform real comparisons;
  generated constant/zero-trim cases do too. Two positive near-degenerate cases
  exercise the short-but-positive length branch. The coincident analytic-line
  fixture intentionally remains a native constructor rejection (`1007`), not a
  fabricated parity result. Rejections fall from 41 to one. Native runner
  regressions verify all five zero/near-degenerate comparisons and the separate
  analytic rejection.
- **Current follow-up checks and upstream limitation.** Formatting, C++-enabled
  workspace/all-target Clippy with warnings denied, 36 p1 kernel tests, 22 p1
  runner tests, 14 p1 FFI tests and wasm checking pass. The newly merged sphere
  module omitted the existing `OffsetSurface` variant from its fallback match;
  the integration fix routes it to the same generic boundary, with a regression
  and an unchanged 800/800 sphere differential result. The full kernel run is
  **not green**: 1352 tests pass and the unchanged upstream
  `surface::surface::p2::tests::read_from_db_unported_types` test fails because it
  still expects the now-ported `OFFSET_SURFACE_TYPE` reader to return `Unported`.
  That unrelated assertion is not changed or skipped, and no full-suite pass is
  claimed. Local compilation now shares 10 Cargo jobserver slots, uses the
  upstream `cc/parallel` feature, persistent compiler caching and Linux-native
  target/cache volumes. A forced 204-shim cached rebuild measured 61.65 seconds
  on the host bind mount versus 5.38 seconds on the Linux volumes; this is a
  warmed FFI rebuild measurement, not an end-to-end cold-suite speedup.
- **Control point magnitude bound (HANG-37; PR #282 fuzz, 2026-10-20).** `--fuzz 2000`
  on the face-tail follow-up produced an `extend` case (`fuzz00331`) whose B-spline
  surface has a control point at `-1e300`: the native `GetCreateExtendedSurface` gave no
  result within 25 s. Native timing with that coordinate replaced, per axis: `1e154`,
  `1e155`, `1e156` return in 25-45 ms on x, y and z; `1e157` and `1e158` give no result
  within 25 s on every axis; `1e160`, `1e180`, `1e200`, `1e250`, `1e300` likewise; the sign
  of a zero distance does not matter. The FFI `points()` guard now refuses control point
  coordinates (and coordinate/weight) above `MAX_COORDINATE = 1e154`, 1000x below the
  first slow value (each coordinate's own square also still stays finite there; a 3-term norm would not); the runner's Rust side runs the
  same check. The input is kept in `crates/difftest/tests/fuzz_regressions/`. Two `differs` notes from the
  same run (`fuzz01001`, `fuzz01653`: a `map_twin` rectangle corner at `±1e-300`, native
  vertex `0` vs Rust `±1e-300`) are the native constructor's rounding of a coordinate
  outside the generator's domain, not a port difference.

- **Vertex-cap review-readiness integration (2026-10-21, PR #288).** Reconciled
  the source with main `159b34d2`, preserving #282's contained-face/mapping work
  and both ordinary/strut vertex-creator calls. ABI modes 4/5/6 coexist without
  changing their payloads: face_tail, map_twin and vertex. The combined fixture
  budget is 448 plus 600 generated cases. Conflicted reports are regenerated
  from the combined native run, never selected or hand-merged.
  P1 is **1048 cases / 731 compared / 316 deferred / 1 rejected / 0 failed**,
  max relative error `1.850371707708594e-16`, sensitivity **731/731**. All 18
  vertex-named cases pass bit for bit. The shared creator's report remains
  byte-identical to main: **874 / 585 compared / 289 deferred / 0 failed**,
  sensitivity **585/585**. No tolerance, seed, generator or absolute floor changed.
  Formatting/include checks, strict Clippy with/without C++, workspace tests
  (**1559 kernel tests**), wasm, **27 p1 runner tests** and **16 FFI tests** pass.
  The full C++-enabled runner library also passes: 179 passed, 29 ignored;
  float-roundtrip and the three runner integration tests pass as well.
  The full local ARM64 gate completes **238 units: 118 green, 116 partial and
  the four documented platform-dependent hash/healing failures**; canonical
  x86-64 CI remains the published-head gate.
  Independent source review found no blockers in typed arena routing, tolerance
  selection, ownership transfer or shared creator delegation. Sixteen added
  vertex fixtures include fourteen actual caps and two no-cap filters, not just
  surface-presence checks. This is still a **partial** executive: general
  three-edge trims, threshold-sensitive routing, nonempty trim production,
  closed-surface seam mutation and the full FaceuseOffset/EdgeuseOffset/SSI
  bridge remain outside this vertex follow-up. This branch is not stacked on
  #201 and makes no new shared-native-context synchronization guarantee.
  The kernel-dispatch API migration threads the actual caller kernel through
  vertex fixtures and evaluations; numerical formulas and both native reports
  remain unchanged. CI success, not an assertion of complete geometry
  coverage, is the readiness gate under the updated repository policy.

- **Historical vertex-cap executive adapter follow-up (2026-10-20).** Both ordinary and
  exactly-one-boundary strut creator calls now reuse the landed `VertexOffset`
  algorithm through a narrow internal backend. Tagged typed handles distinguish
  original vertices from result edges/faces; selection uses the result tolerance,
  while the creator reads the original vertex's B-rep tolerance. Seam queries borrow
  the real mutable result arena, concrete curve kinds are preserved, and generated
  geometry is moved into the owned output. There is no analytic sphere shortcut.
  Sixteen new fixtures cover isolated vertices, translated/scaled strut caps,
  mate-faceuse fallback and no-cap filters. Snapshots compare surface type/domain,
  full homogeneous NURBS data, knots, second derivatives, and trims/orientations.
  ABI mode 6 leaves modes 4/5 available for the independent face-tail work in #282.
  The unit remains **partial**: 709 cases, 704 compared passes, four deferred,
  one native constructor rejection, zero failures; maximum relative error remains
  `2.9158035700250084e-14`, unchanged `1e-9` tolerance, no absolute floors.
  All 18 vertex-named cases compare bit for bit. Remaining first stops are
  `CalculateBoundingBox`, editing metadata, `ShrinkGeometry` and the legacy
  normalized-evaluation adapter. The old `RemoveEmpty` test expectation is corrected
  to the actual part 1 editing-metadata boundary rather than part 5 removal.
  After merging main `aabae414`, formatting, strict C++-enabled Clippy, default
  workspace tests (1501 kernel tests), wasm checking and the complete p1/creator
  differential runs pass locally in Docker. Sensitivity detects 704/704 p1 and
  428/428 creator comparisons; the creator report remains unchanged. Before that
  merge, the 16 p1 FFI tests and 24 p1 runner tests passed serially, and the p1
  report also passed against the x86-64 C++ reference. Independent GPT-6-Astra
  review found no remaining code blockers for a **draft** PR. Executive-level
  ordinary multi-boundary caps, threshold-sensitive tolerance routing, actual
  nonempty trim production, and analytic/closed-surface seam paths remain coverage
  gaps. Empty trim snapshots do not certify those paths.
  **Full canonical x86-64 manifest validation is pending CI.** The local ARM
  campaign on the earlier `ef504a4a` base ran all 228 selected units but reproduced
  the four documented architecture-specific hash/healing failures (General above).
  Local x86-64 emulation timed out before completing the full gate; no full-gate
  pass is claimed. Shared native synchronization remains separate work in #201;
  this main-based follow-up retains main's native calling convention and makes no
  new cross-wrapper thread-safety guarantee.
