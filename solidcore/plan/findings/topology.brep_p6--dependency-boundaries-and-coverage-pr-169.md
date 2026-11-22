# topology.brep_p6 — dependency boundaries and coverage (PR #169)

This port builds on the existing `topology.shell` PR #154, including its real
region/shell records and mutation helpers. It reuses the traverser's co-ported
`SmFace::HasLaminaEdge`; it does not re-port the open face or B-rep dependencies.

The current gate is partial. `CopyEdges` trims the first source edge in the
**destination** context, then reaches `CreateWireEdgeFromCurve` (`brep_p5`).
`MakeFaceTopology` checks the region/surface and ordered input loops, allocates
its partial face/use records, and reaches the active `SM_USE_OLDTOL`
`SmFace::SetTolerance` call (`face_p3`) before inserting face uses or building
loops, radial sectors or trims. `TestClosureSplitRegion` handles separated mates,
lamina edges and open traversals; a closed traversal creates a solid region and
shell, transfers the connected face uses in native order and reaches
`SmBrep::Notify` (`brep_p2`). The dependent remainder of those algorithms remains
deferred. No completed copy, successful face construction, region split, radial
sector recovery, trim generation or ray classification is certified here.

Coverage correction after review: the original 369 successful op-0 fixtures
all return before taking a mark lock, because the mate is in another shell or
the seed has a lamina edge. Their zero context deltas and cleared marks do not
certify traversal or lock release. Six tiny plane cases are edgeless, not lamina,
and defer at `Notify` after closed traversal; they do not establish split parity.

`open_traversal.json` adds 56 cases over the actual five-face sheet in
`open_box.smb` (provenance and generation recipe in the fixture README). Seeds
0 and 5 share their mate's shell and have no lamina edge; traversal reaches the
hole through adjacent faces and returns open. Native FFI regressions assert a
nonzero MARK advance (4 per call), visited faceuse marks and a released operation
lock. Differential comparisons cover both seeds, held caller locks, repeat counts
and infinite-region flags, comparing all five mark classes and shell links.

All `MakeFaceTopology` shim inputs have empty loop, edgeuse and loop-vertex lists
and no explicit region. Consequently its loop validation and existing-shell
branches are not compared; non-null surfaces always defer at `SetTolerance`.
Rust-only loop regressions do not change that native coverage limitation.
Other passing fixtures compare two empty copies and 16 null-surface operation
errors with preseeded outputs reset. Inputs also include 15 upstream `.smb`
B-reps, analytic planes, actual curved edges and scaled B-spline faces. Large
B-spline fixtures keep UV domains inside their natural domain. Nonempty copies,
non-null face construction and completed splits are deferred. No absolute floors.

The runner uses the native graph identity encoding consistently: null is -1,
original objects use their traversal-order index, and shells/regions created
after the input walk are -3. This includes output pointers and faceuse shell
links, so future `brep_p2` integration cannot expose arena indices as identities.

Rust regressions check extreme loop counts without allocation from those counts,
and release of the operation's mark lock after a dependency error while preserving
the caller's lock and the already performed shell mutation. Native FFI tests
exercise invalid selectors/flags and accept a valid 65-entry duplicate-edge copy
list, including compound candidate-mark masks 3, 7 and 31. The native mark-lock
constructor accepts OR-ed bits, so the FFI guard accepts all masks 0..=31; zero
requests no outer lock. A fixture policy must not reject these valid inputs.

Integrating shell state with the newer `boolean.merge_p1` exposed five existing
region-color fixtures that lacked a borrowed `RegionLookup`: C++ succeeded while
Rust returned `InvalidInput("region lookup context required")`. Region propagation
now forwards the caller's lookup into `a_object::notify_with_regions`. The runner
maps its native callback keys to the actual imported regions in the two operand
arenas and applies the same `SetIsVoid` as C++. It does not infer region state
from attributes. These are integration fixes, not upstream defects.

The empty destination fixture explicitly includes the native constructor's void
infinite region (`SmBrep.cpp:549-552`). Its region count, infinite-region identity
and raw void flag are read back from C++ and compared with the Rust fixture;
source and destination have distinct caller-owned object keys. Null-surface face
fixtures start from a populated B-rep, so every optional native output can be
preseeded with a non-null object before checking its reset.

The whole-manifest run exposed the existing **HANG-15** cost in
`topology.edgeuse_p2`, including `bs1_cs0_1e-11_1` from `adversarial.json`. A debugger
stack shows `CheckAndSetUVTrimCurve -> CreateUVTrimCurve -> DropCurve ->
SmSurfaceCache::SubdivideToTolerances` about twenty levels deep. An isolated case
finishes as deferred on `intersect.curve_class_p4`; the full 4583-case unit exceeds
a 60-second diagnostic timeout. No comparisons or fixtures were removed to hide
this cost. Progress tracing for the full run is external GDB instrumentation,
without changes to the test harness or its tolerance policy.
The complete `topology.edgeuse_p2` run subsequently finished: 4583 cases,
3218 passed, 1365 deferred, 0 failed, maximum relative error 0.

The complete integrated manifest run finished with 197 units: 109 passed,
88 partial, 0 failed. It includes main `7f760470`; the subsequent `4757b211`
only regenerates `PROGRESS.md`. The guard-only compound-mask extension was
additionally checked with native FFI regressions, C++ Clippy, the standard
442-case brep_p6 run and 394/394 detected sensitivity perturbations. It leaves
all existing manifest inputs and operation results unchanged.

`SmHealData::Fix_BadSheets` calls `TestClosureSplitRegion` without checking its
status (`SmHealData.cpp:4832`) and consumes its partial outputs. The original
adapter's `?` would propagate a later native error and lose those outputs once
the remaining split dependencies were wired. `RegionSplitAttempt` now keeps
the native `SmStatus` beside its shell/region writes. The healer ignores only
that status; missing dependencies and Rust link/resolver refusals remain outer
errors. Missing split links are representation errors rather than invented
null-pointer statuses that an unchecked caller could accidentally ignore.
The differential runner also retains the split outputs on a native error.

A direct FFI regression reaches a real native error on a tiny edgeless plane
and verifies that both allocated outputs remain non-null: the pre-operation
graph encodes them as -3, distinct from NULL (-1). Native errors therefore must
not be replaced by empty output lists. Kernel regressions check partial output
retention, outer propagation of `Unported`/typed-resolver failures, and rejection
of missing split links. Completed region splits remain deferred and uncertified.


### PR #169 review correction validation

On main `52ab9e2f` plus shell dependency `9af40484`, the focused run is
498 cases / 450 pass / 48 deferred / 0 failed, maximum relative error 0;
all 450 perturbations are detected. The 56 new open-sheet cases all pass.
The six affected differential units pass (healer 493/265/228,
merge 248/175/73, traverser 909/889/20, shell 911/892/19,
rotational sweep 761/718/43; counts are total/pass/deferred, zero failures).
Formatting, both strict Clippy configurations, 1277 kernel tests, wasm32 and
four native FFI regressions pass. A new complete manifest gate is required;
earlier 197-unit results above are historical, not evidence for these changes.

Dependency #154 merged first (2026-10-18); main was then merged into PR #169,
and the bundled copy of #154's code now resolves to main's version.

### PR #169 integration with main `8126545d`

The merge preserves both the brep_p6 findings and main's face_p1 findings.
`FlipFaceOrientation` and `CalculateUVDomainFromUVTrimCurves` are now wired:
the traverser's five and shell's four affected deferrals reach `face_p3`
instead of `face_p1`. They remain deferred, not newly certified comparisons.
The shell integration's seven passing traverser cases remain passing.
Regenerated differential reports confirm those dependency changes.

Formatting, strict default and cpp Clippy, 1288 kernel tests, wasm32 and
four native brep_p6 FFI regressions pass. All twelve affected differential
units pass with zero failures; brep_p6 remains 498/450/48 and traverser
909/889/20 (total/pass/deferred). Perturbations detect 450/450 brep_p6 and
889/889 traverser comparisons, with no empty outputs. The complete manifest
gate passes: 201 units, 109 green, 92 partial, zero failures. The coverage
limitations above remain; merging main does not certify deferred paths.
