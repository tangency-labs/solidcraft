# offset.offset_executive_p2 (partial, PR #201, 2026-10-17)

### Merge integration after main c338131c and landed #288 (2026-10-21; current evidence)

- Retained both ordinary and strut vertex-cap creator calls through #288's shared
  typed original/result arena adapter, alongside p2 and the live-kernel notification
  and native synchronization changes. Modes 4/5/6 retain their separate payloads.
- Adapted the face-from-curves wrapper to main's mutable-kernel topology constructor.
  The p6 constructor and closure splitter share their existing implementation through
  reborrowed notification state; marks and geometry use the actual kernel, with its
  context borrowed only at notification calls. Explicit external/null notification
  contexts remain distinct. The owner-key regression now checks successful face
  creation and routing to the selected context after the old tolerance barrier landed.
- Formatting and strict C++-enabled workspace/all-target Clippy pass, as do all
  **1680 kernel tests**. Seven affected Rust/C++ units pass with **4142 total cases,
  2928 compared passes, 1193 deferred, 21 rejected, zero failures**. No cases,
  tolerances, absolute floors, seeds or ordering changed from the combined fixtures.
- P1 is **1048 total, 732 compared, 315 deferred, 1 rejected, zero failures**,
  maximum relative error `1.850371707708594e-16`; the previous isolated-vertex
  adapter deferral now compares. P2 remains **322 / 141 compared / 164 deferred /
  17 rejected / zero failures**. The shared creator and B-rep p6/p7/p9 and Boolean
  p4 reports match merged main. Reports come from this combined native run. Full CI
  remains the merge gate; no local full-manifest or fresh sensitivity run is claimed.


### Review follow-up after main ca3bbd2b (2026-10-21; historical evidence)

- Integrated #282's contained-face tail and notification-aware MapEdgeuses, with
  one shared mapping algorithm for legacy external contexts and the live-kernel
  wrapper. Caller records and source/result identities remain distinct; kernel
  context is borrowed only at notification boundaries, not replaced for geometry.
- The landed #229 constructor now supplies automatic/explicit tolerances without
  resetting adapter options, flags or relations. Namespace-backed construction
  remains valid; new wire/face-copy callbacks use the actual caller key provider.
- Ported the original/extended/outside trim-domain branch, with unrestricted trim
  bounds, EFF_ZERO containment and restoration before SM_ERR. Restoration errors
  take precedence. Result faces use owning-list order and the active traversal
  mark; marked/closing faces are skipped, and eligible faces are marked before
  extension or replacement can fail.
- Tests cover live/external/absent callback routing, retained caller state,
  constructor equivalence, trim boundaries/restoration-error precedence, marked
  skips and marks retained after real notification deferral. The initial
  SmFace::Notify boundary still prevents live swap/restoration comparisons; no
  constructive-shape parity is inferred from these tests. The 100 FaceuseOffset
  bridge deferrals remain explicit, coordinated with #288's separate vertex scope.
- Native p1: **1032 cases, 715 compared, 316 deferred, 1 rejected, 0 failed**,
  max_rel_err=1.850371707708594e-16; sensitivity **715/715**. Its main fixture
  additions are retained and the conflicted report is regenerated. The 312
  face-tail deferrals stop at MakeWireEdgeVertex in topology.brep_p3.
- Native p2 remains byte-identical: **322 cases, 141 compared, 164 deferred,
  17 rejected, 0 failed**; sensitivity **141/141**. Boolean p2/p3/p4 reports are
  also unchanged; p2 and p4 sensitivities detect all 30/30 comparisons each.
- Formatting/include checks, strict Clippy with/without C++, workspace tests
  (**1617 kernel**), wasm, **43 offset runner tests**, **35 offset FFI tests** and
  both native-lock probes pass. The full local ARM64 gate runs **239 units**:
  115 green, 120 partial, only the four documented hash/healing platform failures.
  Canonical x86-64 CI is required for the published head.
- The newly merged handwritten boolean_merge_p3 export now holds the shared native
  lock across native work, destruction and catch handling. No shared C++ reference
  rebuild is needed. This PR's box-construction finding is now WR-209; main's
  independent WR-167 is preserved.

### Prior merged-main evidence (2026-10-21)

- Main through `1a146028` is merged without rebasing. The landed B-rep p2
  manifold, notification, shell-vertex and surface-replacement calls are wired;
  extension still explicitly defers before the unfinished trim-domain continuation.
- Default native run: **322 cases, 141 compared passes, 164 deferred, 17 rejected,
  zero failures**; maximum relative error zero and sensitivity **141/141**.
  All 129 earlier compared records and all 17 constructor rejections are unchanged.
  Twelve former deferrals now compare the strict shell-radius error, not successful
  constructive geometry. Tolerances, generators, seeds and absolute floors are unchanged.
- Current deferrals: executive FaceuseOffset geometry/SSI bridge 100; copy
  constructor (`topology.brep_p3`) 32; `MakeWireEdgeVertex` (same unit) 15;
  `CalculateBoundingBox` (`query.brep`) 7; `ShrinkGeometry` (`topology.brep_p4`) 10.
  The production OffsetBrep bridge, automatic Boolean tolerances (#229), vertex
  follow-up (#288) and complete returned-shape adapter remain separate work.
- Newly reachable p7 glue now retains caller stores/context for incident-face
  PreEdit and includes the real B-rep identity in edge/face notification payloads.
  A regression asserts the complete user/system callback prefix on a real
  triangular sheet, error-path record/attribute preservation and null-context
  semantics. Restoring the original notification behavior makes that test fail.
- The full local ARM64 differential gate ran all **235 units**: 113 green,
  118 partial and the four previously documented architecture-dependent failures
  (`geom.hash_table`, `heal.heal_data_p1/p2/p3`; see General). P1, Boolean p4,
  B-rep p7/p9 and edge reports are byte-identical to their committed reports.
  This is not an x86-64 full-gate pass; fresh canonical CI is still required.

### Earlier scope and evidence history

The prerequisite p1 PR #175 merged on 2026-10-18; its reviewed changes and current
main through `fb58ea76`, including merged batch #269, are integrated here.
All six owned static/member/API entrypoints have real native dispatch and Rust
executable prefixes. The remaining P2-owned continuations are documented behind
actual missing callees, not represented by fabricated topology copies or empty
successful offset/shell geometry.

- **Final integrated native evidence.** After review corrections and the
  notification-aware integration of main `fb58ea76` / batch #269, the default
  unfiltered run completes with 304 cases
  (64 fixtures and 240 fixed-seed generated cases; default CLI seeds remain 16):
  120 passed, 0 failed, 8 native constructor rejections, 176 deferred. All 120
  passes compare numerical values; maximum relative error is zero, with the
  unchanged `1e-9` relative gate and no absolute floors. Sensitivity detects all
  120 perturbations. Rejected constructors are not parity passes. The standard generated report is
  `reports/difftest/offset.offset_executive_p2.json`. These totals supersede the
  earlier pre-guard run. Validation uses pinned upstream `c9c5979`, Rust 1.98.0
  and GNU g++ 12.2.0 on Linux ARM64 in local Docker, with the user-approved ten
  Cargo jobs on the resized 12-CPU/16-GiB Colima VM; builds remain sequential
  under `nice -n 10`, using the cached native reference.
- **Exact certified scope.** Comparable operations are member null errors
  (`1006`), API null errors (`1007`), and API-shell foreign/null selector errors
  (`1007`), including mixed/duplicate source-face selections. Snapshots compare
  exposed executive scalars/flags,
  Boolean state, ordered selector origins (foreign face zero is not source face
  zero), ownership/presence and input purity. Ordinary operation statuses are
  integer observations, not harness errors or fixture rejections. The shim calls
  each named upstream operation; it does not replay native results on the Rust
  side. No successful constructive offset or shell result is certified.
- **Geometry projection limits.** Wire inputs compare natural/trim intervals,
  endpoint evaluations and vertex bounds; sheet inputs compare domains, analytic
  kinds and evaluated points/normals. Boxes compare face/edge/vertex counts,
  analytic kinds, vertex bounds, provenance and purity only. Full native-versus-
  Rust traversal adjacency, stitched face correspondence, region metadata,
  effective per-entity tolerances and arbitrary returned-shape equivalence are
  not established. Both sides independently check that their full observable
  input blocks/arenas remain unchanged on the compared early returns.
- **Guards and ordering.** Public offsets remain signed (`fabs` plus `SetInset`);
  internal member geometry magnitudes are nonnegative. Finite extreme distances
  whose fivefold product overflows still reach native null/invalid-selection
  early returns, which do not use that arithmetic. Other scalar/index/resource
  guards remain active, including malformed selectors after the first invalid
  API selector. Static null inputs stay rejected because upstream dereferences
  them. Direct native and runner regressions exercise these distinctions.
  `ShellBrep` still queries manifold status before its strict radius guard;
  valid API shell selectors still require the real copy before shelling; empty
  rounded offset still calls `ShrinkGeometry`, even when skip is requested.
- **Measured dependency boundaries.** The final report records:

  | Dependency | Cases | First missing call(s) |
  |---|---:|---|
  | `offset.offset_executive_p2` | 67 | Executive FaceuseOffset arena/geometry/SSI adapter; creator is landed |
  | `topology.brep_p2` | 65 | `IsManifoldSolid` (58), `MakeShellVertexTopology` (4), `Notify` (3) |
  | `topology.brep_p3` | 32 | `SmBrep::SmBrep` copy constructor |
  | `topology.brep_p4` | 9 | `SmBrep::ShrinkGeometry` |
  | `query.brep` | 3 | `SmBrep::CalculateBoundingBox` |

  No `UNPORTED(offset.offset_executive_p2)` back-edge callers were present to
  wire. Completing dependencies requires implementing the documented P2
  continuations as well as replacing boundary stubs. The runner deliberately
  fails closed on newly reachable non-deferred results/temporaries until their
  output adapter is extended; its constructive-coverage assertions must evolve
  with that work.
- **Native observer concurrency corrected before publication.** The adapter's
  scoped allocation observer installs a stack callback on the process-global
  API context. A p2-only mutex or serial tests would not prevent another wrapper
  from using that context and having its live B-reps reclaimed by the observer.
  One out-of-line recursive mutex now protects common `SC_TRY` entrypoints and
  handwritten entrypoints through all native destruction, callback restoration
  and catch cleanup. Deterministic probes check exclusion, nested shim calls and
  exception release. A six-caller regression mixes p2 with independently compiled
  `geom.object` box construction/destruction and forces p2 buffer retries.
  Static cross-wrapper/lifetime review found no remaining blocker. The guard
  serializes individual native exports, not unrelated retained-result protocols
  such as SSI's pre-existing `intersect`/`take` pair.
- **Initial broader integrated checks.** Formatting, C++-enabled workspace/all-target
  Clippy with warnings denied, and `wasm32-unknown-unknown` checking passed.
  All 1342 kernel tests pass. FFI tests: 92 passed, 1 existing subprocess-only
  helper ignored, 0 failed. Runner library tests: 152 passed, 22 existing ignored,
  0 failed, including 13 p2 runner tests. A default differential sweep of all
  15 explicitly guarded units plus harness selftest, context/object and p1/p2
  completes: 20 units, 6 green and 14 partial, zero failures. P1 remains
  691 cases / 633 passed / 17 deferred / 41 rejected with
  `max_rel_err=2.5139756042650037e-14`; its generated report is byte-identical to
  the committed report. This is not a completed full `--green` baseline; the
  earlier p1 attempt's 1200-second timeout remains documented above.
- **P1 non-blocking follow-ups carried forward (2026-10-18).** After merging
  p1's reviewed changes and upstream through `59d486b4`, formatting, C++-enabled
  all-target workspace Clippy and wasm checking pass. All 60 offset kernel tests,
  35 offset runner tests, 28 offset FFI tests and two native-guard probes pass,
  including concurrent observer isolation. P1 now compares 675 of 693 cases,
  with 17 deferred, one explicit constructor rejection and zero failures; the
  updated report matches the one committed through p1. P2 remains 118 passed /
  176 deferred / zero failures across 294 cases, its report is byte-identical,
  and sensitivity still detects 118/118 perturbations. No constructive scope is
  promoted. At that stage the separate upstream reader-test baseline failure
  documented in p1's notes prevented a full kernel pass; see the resolution below.
- **Reader CI fix synchronized from green p1 (2026-10-18).** Merged p1 head
  `ca0862c8`, including the upstream #221 correction to the offset/sphere reader
  regression. The formerly failing test now passes, and the full local workspace
  test run is green, including all 1377 kernel tests. Formatting, C++-enabled
  workspace/all-target Clippy, wasm, 28 offset FFI tests and two native-guard
  probes also pass. P1 and p2 default differential reports remain byte-identical
  to their committed reports (675 / 17 / 1 and 118 / 176 / 0 passed / deferred /
  rejected, respectively), with zero failures; p2 sensitivity remains 118/118.
  This resolves the stale unit-test failure without changing offset numerics,
  tolerances or scope. A full local native `--green` run is not claimed; fresh
  p2 GitHub CI is triggered by the synchronization push.
- **Merged-main CI compatibility fix (2026-10-18).** CI run `39085584151`
  compiled its synthetic merge with newer main and failed with `E0063`: p2's
  `Region` test literal lacked the newly introduced `shells` and `is_void` fields.
  The C++-only runner contained two more stale literals. All three now preserve
  `in_brep: true` and use the constructor defaults; fixture shells are linked
  reciprocally from their regions. Regression tests cover the kernel fixture and
  empty/wire/sheet/box runner fixtures without inferring void/infinite-region
  identity or claiming full shell-member traversal parity.
  Main also made face-curve/tangency queries mutable for lazy UV trim geometry.
  The p2 offset/member APIs and runner now borrow the same original arena mutably,
  forward the caller's kernel and collect face IDs in arena order before traversal;
  no topology clone, replacement arena or constructive result is fabricated.
  API-shell selection validation remains immutable. Existing before/after purity
  and origin-qualified selection comparisons remain intact.
  Validation on merged main `3604a8dc`: formatting, workspace/all-target Clippy
  both with and without C++, all workspace tests (including 1389 kernel tests,
  using the optimized profile with debug assertions and overflow checks retained),
  and wasm checking pass. C++-enabled library tests pass: FFI 104 passed / 1 ignored;
  runner 159 passed / 23 ignored. P1's report is byte-identical (675 passed,
  17 deferred, 1 rejected); p2 remains 118 passed / 176 deferred / zero failures
  across 294 cases. Four generated cases (`gen0022`, `gen0023`, `gen0223`,
  `gen0232`) now advance from `SmEdgeuse::NormalizedEvaluate` to
  `SmBrep::GetInfiniteRegion`; the report at that revision reflected dependency
  progress, not newly certified constructive geometry. Sensitivity detects all
  675 p1 and 118 p2 perturbations. A separate native sweep of `harness.selftest`,
  `topology.edgeuse_p2`, `topology.edgeuse_p3`, `topology.face_p2` and
  `topology.shell` also passes at each unit's documented green/partial scope:
  13003 cases, zero failures. Builds use ten jobs and persistent Linux-native
  caches in the resized local Docker VM. A full local native `--green` sweep is
  not claimed; the push triggers fresh full GitHub CI on this merged tree.
- **Review corrections (2026-10-19).** The stale `SmRegion::IsVoid` boundary now
  calls the already-ported getter and tests `!= 0`, preserving C++ truthiness for
  `FALSE` (0), `TRUE` (1), `UNSURE` (2) and other nonzero values. Tests distinguish
  each mate direction, inset, rounded identity selection and invalid region IDs.
  Input fixtures explicitly designate the constructor's infinite region with
  `is_void = 1`; interior box regions retain the non-void constructor default.
  This replaces the earlier compile-compatibility-only fixture initialization.
  The 44 former void-getter deferrals now reach `FaceuseOffset`.
  Newly merged main also ports infinite-region lookup and `MakeFaceWithCurves`.
  P2 calls those real APIs, initializes its empty temporary's single void infinite
  region and editing flag as upstream does, and restores the temporary before
  propagating construction errors. The 23 former rounded identity deferrals now
  reach `FaceuseOffset`; four copied-face cases reach `MakeShellVertex` through
  p7. This advances genuine boundaries, not constructive parity.
- **Incomplete native box construction (review must-fix 2).** `CreateBox` ignores
  face-construction statuses and its API may report success for incomplete
  topology (WR-209 in `plan/upstream-issues.md`). The adapter now verifies actual
  face/edge/vertex counts of 6/12/8 immediately after construction, before selector
  resolution or operation execution, and rejects otherwise with `SM_ERR` (1001).
  It uses no size cutoff. Eight new fixtures cover both reported scales, source
  and foreign boxes, and null/indexed selectors; two valid-box controls still
  compare actual API invalid-selection status 1007, geometry and purity.
  FFI regressions also verify cleanup via subsequent valid calls and ensure
  malformed transport input remains `SC_ERR_ARG`, not a constructor rejection.
- **Shared edge prefix (review should-fix).** P1 and p2 now share
  `prepare_offset_edgeuses` and `fill_offset_edgeuse_gaps`. Empty synchronized
  lists execute the real gap path instead of unconditionally deferring on
  `EdgeuseOffset`; validation and error order are unchanged, including both gap
  calls executing before either status is inspected. Public p1 still performs
  edge/vertex notifications using real records and distinct arena identities,
  covered by an extended notification regression. P2's currently unreachable
  live-pair continuation still requires caller-owned notification state; no dummy
  `NotifyEnv`, context or attribute store was introduced to force full delegation.
  The shared native mutex is retained unchanged: observer safety requires
  exclusion across wrappers, not a p2-only lock.
- **Review-fix validation on main `b6b6f7dd`.** Formatting (including direct checks
  of include-generated runner/FFI modules), Clippy with and without C++, wasm,
  all workspace tests (1417 kernel tests, including 70 offset tests) pass.
  Tests use the optimized profile with debug assertions/overflow checks retained.
  C++ library tests: 107 FFI passed / 1 ignored, 162 runner passed / 24 ignored;
  the targeted offset subsets are 31 and 37 passes respectively. P1's generated
  report is byte-identical (675 passed, 17 deferred, 1 rejected), with 675/675
  perturbations detected; p2 reports 120 passed, 176 deferred, 8 rejected across
  304 cases, with 120/120 perturbations detected. All have zero failures and
  unchanged tolerances. A native sweep of `topology.brep_p1`, `topology.brep_p7`,
  `topology.shell` and `topology.face_p2` also passes at partial scope: 6508 cases,
  zero failures. The previous head `23db8234` passed full GitHub CI run
  `39119513799`, including `--green`; no full local `--green` run on this new
  revision is claimed. Fresh CI is required after pushing these corrections.
- **Notification-aware batch #269 integration (main `fb58ea76`).** #269 initially
  excluded #201 because real splitting now mutates and notifies topology, making
  the earlier notification-free preparation contract invalid. P2 now requires
  caller-owned `OffsetNotifications` on all six public Rust entrypoints: actual
  topology/attribute stores, disjoint original/result keys, result-curve identities
  and a reserved result-Brep key. The runner registers its constructed inputs in
  separate source/foreign/result/curve namespaces before taking purity snapshots.
  No empty substitute environment is created by the library.
  `NotificationStores` carries data without borrowing a context. A shared internal
  `NotifyState` binds the actual `kernel.context` briefly at notification sites,
  allowing geometry to use that same kernel between calls. No context is moved
  out, replaced or cloned. Legacy p1/p7/p9 `NotifyEnv` entrypoints retain their
  external `None`/`Some` behavior and share the same algorithms with the new
  kernel-context entrypoints. Split Boolean scopes preserve one real context in
  kernel mode, and retain the legacy two-context restoration when external.
  P2 passes the existing claimed mark through both mapped-edge loops and calls
  p1's real builder once, rather than preparing/splitting twice or omitting Copy
  notifications. Tests cover actual user/system callbacks and their order,
  restoration on errors, real attribute copying, result-only split identities,
  nested marks and null/invalid-selection precedence without touching callbacks.
- **Traversal, construction and landed callees.** P2 visits source faces through
  native `GetFaces` list order, with a reordered-list regression, rather than arena
  insertion order. Its temporary reuses `Brep::new_with_infinite_region()` and
  receives the caller's result key before construction, plus the executive's
  tolerance/editing metadata. The historical constructor subset is recorded in
  brep-p3's co-port notes while preserving p10's existing attribution.
  Copied faces invoke p7 using real stores and the same kernel context; extended
  Copy notification is wired before faceuse mapping. Kernel-context excess-face
  removal calls real p5 removal, preserving ignored ordinary errors; final solid
  stitching uses the landed implementation. New first-stop migrations are
  reflected in the dependency table above. Generic geometry-creation algorithms
  are available, but the executive's original/result arena and SSI adapter is
  deliberately still partial and now attributed to p2 itself (67 cases), not to
  a supposedly absent creator. Completing that bridge and the documented offset
  assembly continuations remains a separate task; no constructive parity is added.
- **Native observer compatibility.** Two newly merged handwritten exports bypassed
  the common `SC_TRY` lock: `sc_blend_fillet_intersector_run` and
  `sc_surface_cone_p3_intersect`. Both now acquire `SC_NATIVE_LOCK` outside their
  `try` blocks, retaining exclusion through native destruction and catch cleanup.
  Their numerical reports remain byte-identical. The tiny-box rejection remains
  owned by the allocation observer during unwinding; no redundant delete is added.
- **Validation of this integration.** Formatting, Clippy with/without C++, full
  default-feature workspace tests (1485 kernel tests), and wasm checking pass.
  Tests use the optimized profile with debug assertions/overflow checks retained.
  All 114 C++ FFI library tests pass (one existing ignored). Offset subsets pass:
  75 kernel, 38 runner and 31 FFI tests. P1 difftest: 688 passed / four deferred /
  one rejected across 693 cases, max relative error `2.9158035700250084e-14`,
  sensitivity 688/688. P2 remains 120 passed / 176 deferred / eight rejected across
  304 cases, maximum relative error zero, sensitivity 120/120. All offset runs
  have zero failures, unchanged tolerances and no absolute floors.
  Seven native dependency units (`topology.edge`, brep p7/p9, offset geometry
  creation, stitch p2, fillet intersector and cone p3) total 6905 cases, zero
  failures, and reports byte-identical to the incoming main reports. Two native
  lock probes and the mixed allocation-observer concurrency regression also pass.
  **The broad C++-enabled runner library run is not fully green:** 170 passed,
  one failed, 28 ignored. The unchanged
  `intersect_curve_class_p4::tests::undefined_scripts_are_rejected` assertion at
  runner line 2228 fails for `loop_crossings_props_null_face_1`, also in isolation.
  That runner, its FFI wrapper and native shim are unchanged from main; no assertion
  is weakened, skipped or patched here. No full local native `--green` run is
  claimed for this revision. Fresh full GitHub CI is required after the push.

- **Review correction after main `3132491e` (2026-10-20).** The p7 merge conflict
  is resolved without reverting notification-aware p9 construction. Main's region
  is already optional; both context-aware callsites now forward it directly.
  The shell manifold-difference continuation calls the landed production adapter,
  with negative `(original, shell)` and nonnegative `(shell, original)` order,
  `Difference`, caller tolerance and `20.0 * PI / 180.0`. Caller topology/curve
  identities, actual records/attributes and the live kernel context are threaded
  through the existing adapter, not replaced by namespace-zero stores or a cloned
  context. `OffsetNotifications` additionally requires `original_curve_ref` and
  `boolean_source_lists` (reserved type-zero primary/other region-list handles).
  The original namespace-backed Boolean API is preserved. A record-scope drop
  guard returns partial edits on success, returned error and user-callback unwind.
  Automatic tolerances (`distance < EFF_ZERO`) remain the explicit missing
  `boolean.merge_p2` constructor dependency owned by #229, rather than duplicating
  that claimed constructor or inventing an edge-size tolerance. The borrowed
  cache path stops at `BuildTrees`' genuine missing `CalculateBoundingBox` call;
  future cache wiring still requires live-kernel access. Native Boolean deletion
  of the other operand precedes ignored cache cleanup, so a late deferred exit
  reports the actual surviving Rust operand slots. This is **not** parity with
  the native ownership flag on a path that native code would continue executing.
  No successful constructive Boolean is newly certified.
  The native sheet/wire fixture builder now checks actual projected counts before
  selector resolution: sheet 1/1/4/8/4 and wire 0/0/1/2/2 (faces/loops/edges/uses/
  vertices). Tolerance-driven degeneration becomes a constructor rejection, not a
  mismatched Rust/native scaffold or a parity pass. There is no arbitrary size or
  tolerance cutoff, no shell-count constraint and no independent rejection delete;
  the existing observer owns cleanup. Eighteen fixtures add the two exact review
  reproductions, source/foreign and null/indexed variants, the tolerance-0.5 sheet
  control, a large sheet at tolerance 1e3 and tiny/huge/zero-trim wire controls.
  The new report has **322 cases: 129 passes with values, 176 deferred, 17 rejected,
  zero failures**, maximum relative error zero; sensitivity detects 129/129.
  All original 304 case records are unchanged. P1 (688/4/1-rejected), manifold
  Boolean (30/278/3-rejected) and p7 (197/119) reports are byte-identical, with zero
  failures. P1 and Boolean perturbations detect 688/688 and 30/30 respectively.
  Formatting, strict Clippy with/without C++, default workspace tests (1570 kernel
  tests), wasm, 21 p2 CPP FFI tests and 18 p2 CPP runner tests pass in local Docker.
  Focused kernel tests cover signs including both zeros, identity/attribute routing,
  real callbacks, constructor deferral, late ownership, cache deferral and unwind
  restoration. Independent GPT-6-Astra review found and verified the fix for record
  loss on unwind, then found no remaining code blockers. These latent-path tests
  do not substitute for native successful-shape comparisons; full x86-64 CI remains
  required, and no new local full-manifest or full CPP runner-library pass is claimed.
  **Open executive geometry integration:** `FaceuseOffset`, `EdgeuseOffset` and
  `VertexOffset` need a production `OffsetBrep` bridge over original/result arenas
  and owned geometry/SSI stores. The algorithms in `offset.offset_geometry_creation`
  are landed; the 67 self-attributed p2 deferrals identify the missing bridge, not
  an unported p2 entrypoint. The narrow vertex bridge is a separate PR #288; the
  face/edge construction continuations and fail-closed output adapter still need work.

### Owned face-geometry bridge (PR #452, 2026-10-21)

- Both executive member paths now call the existing `FaceuseOffset` algorithm,
  then the existing p1 face builder. A narrow private backend is shared with the
  public full-backend creator; it does not duplicate offset numerics or provide
  dummy implementations of unrelated `OffsetBrep` operations. Source face/surface
  handles remain distinct from newly owned surfaces and SSI curves. Output slots
  transfer once, preserving concrete kinds, optional UV slots, curve types and
  deviation values. The source arena is borrowed without cloning topology.
- Rounded selection still compares region identity; extended selection still
  tests native void truthiness. Inset now assigns the selected mate. Rounded null
  output skips the face, whereas extended null output and its exact SSI-count
  limits return `SM_ERR`. Both paths retain upstream's `skip_self_intersection =
  TRUE`; the nonempty-SSI transport/count tests are synthetic policy evidence,
  not a claim of native executive self-intersection coverage.
- A thin p1 wrapper reuses the caller's kernel notification callbacks and the
  existing face-construction algorithm. Source and result keys, records and
  attributes remain caller supplied; legacy external/null contexts are unchanged.
  Tests establish callback restoration on no-op/early-error exits and that a
  geometry error precedes builder mutation or notification. Successful
  face-construction notification routing through this new bridge remains blocked
  by the dependencies below; existing p1 mapping tests are supporting evidence,
  not end-to-end constructive certification.
- An unfiltered native comparison against a frozen `5c3adfbe` baseline retains all
  2244 case identities and every prior pass/rejection, with zero failures. The
  creator remains **874 / 585 passed / 289 deferred** and p1 remains **1048 / 732
  passed / 315 deferred / 1 rejected**. P2 remains **322 / 141 passed / 164 deferred /
  17 rejected**: its 100 self-deferrals now reach `SmBrep::MakeWireEdgeVertex`
  (**50**, `topology.brep_p3`) or the non-B-spline `SmCrvOnSurf` constructor
  (**50**, `curve.crv_on_surf`, addressed by the separate #307 work). The complete
  p2 deferred totals are `topology.brep_p3` **97** (32 copies, 65 wire-edge
  constructors), `curve.crv_on_surf` **50**, `topology.brep_p4` **10**, and
  `query.brep` **7**. No tolerance, absolute floor, seed, fixture, native guard or
  rejection policy was changed.
- This is pipeline progress, not a new whole-operation parity pass or a green
  promotion. Full returned-offset topology, subsequent edge/merge continuations,
  and newly exposed dependencies still require their own native evidence. Local
  native runs use pinned Rust 1.98 and GNU C++ 12.2 on Linux ARM64; canonical
  Linux x86-64 CI remains the merge authority, including the documented General
  UB-35/UB-194 platform limitations.

### Face bridge integrated with main `1b7f638e6` (PR #452, 2026-10-22)

- Preserved upstream's per-unit metadata migration and ignored report artifacts.
  The thin p1 wrapper propagates the new notification-lease acquisition `Result`
  before returning the existing builder's inner status; callbacks and their
  identity domain remain supplied by the caller. Independent source review found
  no production-code blocker within the bridge's bounded scope.
- Canonical x86-64 CI run **39738195736**, head `52a2660bf`, passed the complete
  differential gate: **252 units, 134 passed, 118 partial, zero failed**. It also
  passed strict Clippy both with and without C++, wasm, formatting, licensing and
  packaging. The final integrated plain workspace tests pass locally, including
  **1842 kernel tests**. These supersede the old platform-limited gate above.
- Current native summaries are creator **874 / 609 passed / 265 deferred**, p1
  **1048 / 733 passed / 314 deferred / 1 rejected**, and p2 **322 / 141 passed /
  164 deferred / 17 rejected**, all with zero failures. The p2 self-deferral is
  absent: its 100 cases reach `SmSurface::DropCurve` (**50**) or the non-B-spline
  `SmCrvOnSurf` constructor (**50**). Upstream's real wire-edge constructor now
  runs before the former `MakeWireEdgeVertex` boundary's next missing call.
  Complete p2 deferral totals are `intersect.surface_drop_lib_p3` **65**,
  `curve.crv_on_surf` **50**, `topology.brep_p3` **32** (copies),
  `topology.brep_p4` **11**, and `harness:offset-result-adapter` **6**.
- That CI run exposed two stale exact-callee runner-test expectations, not native
  disagreements. Their expected owner/symbol now follow the observed
  `intersect.surface_drop_lib_p3` / `SmSurface::DropCurve` path; no obsolete stub,
  tolerance relaxation, case removal or rejection-policy change is introduced.
  P2 remains partial, with no new whole-operation parity pass claimed.
