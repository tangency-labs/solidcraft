# topology.brep_p9: per-call continuations and coverage limits (PR #183)

### After topology.brep_p1 (#145) merged (2026-10-19)

- **Part 1 wired.** Every former `UNPORTED(topology.brep_p1)` call in p9 is real.
  Part 1's `add_*_record` appends faces, edges and vertices to their owning lists at
  allocation, while native `PostInsert`s at the end of the operation. No same-kind
  record is allocated in between, so the order matches; a record allocated before a
  dependency error stays listed, where native would leak it unlisted. Deleted loops and
  loop uses are reset in place, as part 1 deletes vertex uses.
- **Editing state.** `MakeVertexSplitEdge`, `MakeEdgeInFaceWithVU` and point
  classification read `Brep::editing_enabled`. The offset executive's
  `sm_InsertBreaks` wiring had forced editing on; the native shim's offset B-rep keeps
  the constructor's FALSE, so native refused those splits while Rust attempted them
  and deferred. Reading the B-rep's flag makes the 11 cases compare (offset: 676 to
  687 passed).
- **New p9 callers from main.** `topology.brep_p7`'s `MakeShellVertex` and
  `heal.stitch_p1`'s `MakeVertexSplitEdge` are wired. The stitch's 26 former p9
  deferrals pass as matching refusals; no split mutation is compared there.
- **Coverage.** 684 cases, 539 passed, 145 deferred, 0 failed (head `145e1693`),
  including real-file fixtures and nonfinite point/tolerance cases, with no native
  crash. `FindInfiniteRegion` compares 4 successful results, all through its
  single-region early return. A wire split now runs to its final `SmBrep::Notify`
  (`topology.brep_p2`); no topology mutation is certified yet.

### Issue #210 continuation revision (source `84ee130c`)

All six owned bodies now continue after individual dependency calls. This replaces
all previous `OMITTED` ranges: ray sampling and fallback, marked sheet walk,
vertex-loop creation, split geometry/use rewiring, and edge/loop/face insertion and
inner-loop redistribution. Only official main was merged; active dependency PRs
remain independent. Official main through f30275bb is included; the final merge
changes generated progress only.

Review fixes preserve the base-class DropCurve creating-UV default TRUE and call
existing DropAndTrimCurve with tolerance * 100 and TRUE/TRUE. Defined NaN/Inf
split inputs remain covered; the part-container guard runs before both native
loaders. Explicit NER null-pointer statuses and Unitize status 1007 are retained;
a focused zero-ray regression covers the latter. ComputeLoopOrientation's optional
output and 3D flag are explicit at the dependency call.

Notification storage/identities and actual curve type identities are caller inputs.
The Boolean intersector now invokes p9 through its mutable adapter; typed
point-classification and merge vertex-copy adapters supply notification state.
The opaque classification API lacks mutable owning-arena lookup and defers to
that p1 integration boundary. Canonical editing state remains with p1 (PR #145),
with native constructor default FALSE. No p9 algorithm `UNPORTED` call remains
elsewhere in the kernel.

Source review found InsertManifoldEdgeEnd only setting each VU's vertex, while
SmEdge.cpp:4641-4643 also appends both VUs to the vertex owning list. Both insertions
are restored in order, with a focused start/end regression. Edge skeleton caches
follow the existing importer primary/mate order. The edge differential rerun passes
1715 cases, with 72 deferred and zero failures.

Remaining barriers are individual calls to the active p1/p2 and face/query/drop
owners. Append-only arena retirement is deferred to p1 rather than exposing deleted
loops as live topology. Constructor/debug allocator notifications follow the shared
arena model; constructed topology flag records are registered in caller storage.
The shell retry has at most two passes. Inner-loop transfer must shrink its source
list or return an error; undefined odd edgeuse pairs are rejected. These deviations
and bounds are documented in the source header.

**Coverage limits:** 0/6 owned entrypoints have a compared successful completion;
no p9 topology mutation is certified. Default passes comprise 477 split refusals,
10 insertion guards/refusals, 8 point-class edge refusal paths, 8 point-class vertex
updates and 2 projection helper probes. The 108 deferrals comprise 3 region queries,
3 orientations, 6 shell vertices, 4 vertex loops, 85 splits, 2 insertions, 4 point-class
edge merges and 1 projection probe. Op-5 setup vertex loops are native imports,
not successful Rust MakeVertexLoop calls. UV attachment/redistribution,
notification/attribute construction and the full new mutation tails remain
uncertified until dependencies are wired. The graph oracle is demonstrated for
refusals and vertex updates only; successful mutations still require audit of
legacy caches, stable identity/order normalization and complete curve/UV geometry.
No tolerance changes or p9 absolute floors were introduced.

Revision-specific evidence: Ubuntu 26.04.1 x86_64, GCC 15.2, Rust 1.98.0, pinned
SMLib c9c5979; nice 10 and one Cargo job. The 1268-file source/config/fixture manifest
has SHA-256 `96065a1d187c089bb7e207827f81ed5d2a6dd15492a582f7fcc34192bdad6017`.
The final validation source audit differs only in the intentional modules.toml status
change; final local metadata/report edits do not alter tested functional source.
Passed fmt and direct include-file rustfmt, plain and native strict workspace
Clippy, 1325 kernel tests (0 failed), WASM check and the native part-container guard
(1 passed, 71 filtered). All 12 affected differential runners are partial with
0 failures. P9 default: 613 cases, 505 passed, 108 deferred, max relative error 0;
blockers surface_drop_curve 1, surface_drop_lib_p3 1, brep_p1 96, brep_p2 3,
face_p3 7. Perturbation detects all 505/505 perturbed cases, with 108 deferred.
4096 generated seeds: 4197 cases, 3479 passed, 718 deferred, 0 failed, max error 0.
Full green/partial gate: `difftest: 204 unit(s), 113 passed, 91 partial, 0 failed`. `--status-of topology.brep_p9` returns `partial`.

Native runtime Cargo commands use the CLI-only override
`--config 'profile.difftest.package.difftest.opt-level=0'` with
`run --profile difftest -p difftest --features cpp -- <args>`. Standard runner
opt-level 1 compilation was killed by SIGKILL near the host's 8 GiB limit, even
with one job; the override builds successfully. Kernel-crate and C++ profiles
remain unchanged, though generic code instantiated in the runner may be affected.
Standard-profile native Clippy passed; a standard opt-level 1 executable build
did not complete on this host. Fixtures, comparison rules and tolerances are
unchanged; no workflow/build/Cargo configuration changes are bundled. Earlier
snapshots below are historical and do not certify the full continuation revision.

### Previous c19c0db1 checkpoint (historical)


- **Review status correction (2026-10-18).** The following `47758e96` /
  `c482d6fc` evidence is historical. At that checkpoint the unit was `todo`, because the code after
  the first unavailable call was omitted rather than ported behind per-call
  stubs. `OMITTED SmBrep.cpp:<range>` comments identify the remaining bodies at
  each dependency return. Replacing an individual dependency stub does not finish
  those functions. No successful p9 topology mutation has been compared. The
  insertion fixtures' vertex loops are native setup imported into Rust; they do
  not certify Rust `MakeVertexLoop`. The small PRE_EDIT path was not duplicated
  from active `topology.face_p3` work (PR #139).
- **Review behavior fixes.** The insertion projection prefix now calls the
  existing `Surface::drop_and_trim_curve` with face tolerance times 100 and
  `true, true`, after empty drops at factors 1, 10 and 300. The plane override
  receives the base `SmSurface*` default `bCreatingUVTrimCurve=TRUE`. A separate
  geometry-only prefix probe compares partial overlap over `[-3,7]`, a contained
  drop and the empty-drop fallback without pretending that an edge was inserted.
  The prefix helper is the same code used by insertion. NaN/positive/negative
  infinity split parameters retain their defined native refusals and endpoint
  outputs; six op-4/op-6 differential regressions exercise them. Opaque edge
  handles defer to `topology.brep_p9` until mutable arena context is supplied;
  production callers no longer mistake that gap for invalid input. A unit test
  checks that this deferral preserves classification state. The FFI rejects UB-1
  part-container headers before its shared native loader; `SmallBox.smb` is
  tested only through that guard. Both new include-generated Rust modules are
  checked directly with rustfmt in addition to `cargo fmt --all`.
- **Integration follow-up.** Official main `f75223ad` is merged, preserving newer
  point-classification and merge notes and all new findings. No open PR branch is
  merged. Editing state remains an explicit argument until the canonical part-1
  B-rep field (PR #145) lands; the module-index conflict with other B-rep parts is
  documented for that integration. The initial merged main `f75223ad` failed
  compilation at `sphere.rs:852` (missing `OffsetSurface` match arm). Preliminary
  Linux checks used that one extra arm only in the runner copy. Official main
  `a8958254` (PR #221) then landed the byte-identical arm and corrected the stale
  reader test; it is now merged. The running native gate uses the same numerical
  source as this official fix. Clean validation is complete, with no additional
  source prerequisite remaining (final summaries below).
- **Review-focused evidence.** Numerical source `12b60efa`, linked endpoint test
  fixture `f52eeb29`, projection probe `6d200c86` and doc lint fix `8f11ed4a`:
  `PARTIAL topology.brep_p9 cases=613 passed=505 failed=0 deferred=108 max_rel_err=0e0`.
  All 505 perturbations are detected. The 4096-seed run has 4197 cases, 3479
  comparisons, 718 deferrals and zero failures. All six nonfinite parameter
  cases and both overlapping/contained projection probes pass. The empty-drop
  probe reaches the real DropAndTrimCurve and defers at its deeper
  `SmSurfaceDropCurve::DoTrace` boundary. Ten focused runners have zero failures:
  p9; point class; curve class p3; proto-topology p1/p2/p3; merge p1/p5;
  surface-drop-lib p1; plane p1. Proto p2 is 1247 pass / 286 deferred: its four
  opaque-edge merges now defer instead of being counted as guard matches. The
  other focused counts are unchanged. No successful mutation of the six owned
  p9 entrypoints is compared; the two successful projection probes certify only
  that extracted prefix. The native part-container guard regression passes.
  Plain strict Clippy and WASM checks passed with the former runner prerequisite;
  the pre-main-fix full test rerun was 1317 passed / 1 failed. That failure was main's
  `surface::surface::p2::tests::read_from_db_unported_types`, which still asserts
  that the newly ported OffsetSurface reader returns Unported. Both p9 regressions
  pass. Native-enabled strict workspace Clippy also passed with that now-official
  arm. The complete integration run is recorded below. Clean Clippy passes on
  macOS. Its full test build hit a full disk; package-scoped Cargo cleanup removed
  only this worktree's generated solidcore output. The clean test/WASM/formatting
  rerun is on Linux, which has sufficient space.
- **Clean reruns after main's fix.** No temporary source change remains. The
  sphere source SHA-256 is identical locally and on the runner:
  `8dc0b5917c3a337e07157cdf2774c2c2b433f3d2d7ed47bed31ef7bbb0a462b9`.
  All 1318 kernel tests and documentation checking pass, as do WASM, formatting
  (including direct rustfmt of both new include-generated modules), strict plain
  and native-enabled workspace Clippy and the native part-container guard test.
  The runner's default rustdoc was 1.93.1 while rustc was 1.98.0; the clean run
  selects the installed 1.98.0 toolchain through task-local PATH, without a source
  patch or global configuration change. Clean p9 default/perturbation/4096-seed
  runs reproduce the 505/108 and 3479/718 counts above, bit-identical. The complete
  native gate passes: `difftest: 202 unit(s), 112 passed, 90 partial, 0 failed`.
  Final `--status-of topology.brep_p9` prints `partial`; that describes its tested
  subset, while plan status remains `todo` for its omitted owned bodies.
  A SHA-256 manifest verifies all 1234 tracked source/config/fixture files match
  the runner byte for byte, with no extra source prerequisite:
  `e9d25c655a4d9638e57861d1a6752c2df9664a775ac984ddd5b8807e95d0232a`.

### Previous checkpoint `c482d6fc` (historical)

- **Compared subset.** Numerical source `47758e96`: 604 default cases (92 fixed,
  512 generated), 497 bit-identical comparisons, 107 dependency deferrals, no
  failures and no absolute floors. All 497 perturbations are detected. With 4096
  generated seeds: 4188 cases, 3471 comparisons, 717 deferrals, no failures and
  `max_rel_err=0`. The default deterministic report is committed; the extended run
  uses `--no-report`.
- **Actual behavior covered.** `MakeVertexSplitEdge`'s editing-disabled,
  out-of-domain and nearby-endpoint refusals preserve the returned vertex together
  with `SM_ERR`. Tests cover the strict/scaled parameter boundary, start-before-end
  precedence, real `SmLine`, B-spline and rational/constant curves, opposite primary
  edge uses, non-normalized domains/subdomains and scales from `1e-6` to `1e6`.
  A nonlinear curve with tolerances `[1.15,2]` distinguishes the 2-sample start
  length from its 10-sample recheck and selects the end vertex. Refusal snapshots
  include status/identity and unchanged semantic topology/geometry; these are
  branch/status comparisons, not successful topology-mutation certification.
- **Point classification.** `merge_into_brep` takes an explicit mutable arena,
  resolved target, handle mapping and notification storage. A refused edge split
  updates the class/object fields directly, retains an unset parameter flag and
  returns before SSS relocation/tolerance adjustment. Native fixtures compare this
  early return and vertex-merge positions/tolerances. The legacy opaque-handle API
  cannot resolve an edge's arena and now reports missing caller data; region/face
  owner lookups retain the actual owning units' deferrals. The merge helper with
  existing arena ids calls the real p9 entrypoint.
- **Unimplemented/uncertified continuation.** The missing owning-list operations
  (`GetRegions`, `GetFaces` and default `GetInfiniteRegion`) and UV-trim splitter
  are part 1. `MakeShellVertexTopology` is part 2. Face PRE_EDIT notifications are
  `topology.face_p3`. Generic `DropCurve` is `intersect.surface_drop_lib_p3`, and
  `DropAndTrimCurve` is part 1 of that family. Plane projection calls its existing
  concrete override. These calls stop the owned functions before their remaining
  construction/orientation/ray-selection bodies; those bodies remain integration
  work, rather than being simulated. Interior splitting/insertion defer before
  orphan allocations and native pre-barrier notifications. Owned Rust geometry
  inputs are dropped on errors/deferrals. Supplied UV trims/drop-failure records,
  seam copies, drop retries, optional-shell/bounding-box ray selection, attributed
  and callback-driven construction, late-failure mutation state and successful
  p9 topology mutations are not certified. This unit is partial, not green.
- **Downstream evidence.** All eight affected runners pass with zero failures:
  p9, point classification, curve-class p3, proto-topology p1/p2/p3 and merge p1/p5.
  Proto-topology p2's four newly compared cases are the existing native refusal of
  bare edges without B-rep owners; no native mutation executes there. Its 16 old
  region p9 deferrals now stop at `topology.shell::SmRegion::GetBrep`. Merge p1's
  three old p9 deferrals now stop at the default region (part 1, one case) or
  shell-vertex topology (part 2, two cases). No other unit is promoted.
- **Environment and review.** Clean pinned upstream `c9c5979`, Ubuntu 26.04.1
  x86_64 / GCC 15.2.0 / Rust 1.98.0 on runner-codex-02. The standard optimized
  `difftest` profile and C++ `-O2 -ffp-contract=off -fno-fast-math` are used without
  platform/build/workflow patches or bundled open dependency PRs. Native-enabled
  strict workspace Clippy passes with `--profile difftest`; two concurrent compiler
  jobs exceeded this VM's 8 GiB, so final native checks use one job under nice.
  Plain workspace Clippy, formatting, all 1269 kernel tests and WASM checking pass
  on macOS aarch64 / Rust 1.98.0. Port-pr-review checked the full independent diff,
  owned source prefixes, defaults, status/output lifecycle, guard coverage,
  ownership tags, resource bounds and explicit remaining coverage. Source review
  fixed the endpoint parameter-flag lifecycle and insertion output contract.
  It also removed the FFI wrapper's unjustified sign restriction: finite negative
  raw tolerances are defined because `SmZoneTol3d` applies `fabs`. Two native
  regressions compare that conversion for a B-spline and real line.
  Later source edits only clarify comments; reports/notes record the tested
  numerical checkpoint.
- **Final integration gate (PR #183).** The complete native `--green` run passes:
  `difftest: 198 unit(s), 108 passed, 90 partial, 0 failed`. The final
  `--status-of topology.brep_p9` reports `partial`. All eight affected reports
  regenerate byte-identical to the committed focused reports. This gate uses the
  independent branch through official main `b4765734` and the functional source
  checkpoint above; the final commit adds validation documentation only.
