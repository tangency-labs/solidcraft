# primitive.primitive_creation_p1 — second review integration (PR #213, 2026-10-20)

Merged canonical main `95b823ed`; both conflict sections above are retained.
The three stale `topology.brep_p5` adapters now call the landed implementation
with upstream defaults: `StitchAndOrientOptions::default()`, `Transform` with
no scale and tolerance scaling enabled, and topological cleanup with neither
keep list. Existing caller status handling is retained (ignored statuses versus
`SER`), and pole-free sphere construction now borrows the mutable Kernel needed
by `Transform`.

Main's current `MakeFaceWithCurves` also requires a notification environment.
Cylindrical-box, pole-free sphere and planar-boolean APIs now forward the caller's
records, attributes and identity resolver; the surface owner uses that same face
key. The differential fixture owns its explicit, distinct topology keys and creates
a real infinite region before passing that region into primitive construction;
main p9 now validates it before the missing topology allocation. No throwaway
notification store is created in library code.

`IntersectInsertRelate` and `DeleteFaces` remain deferred: their generic p2
implementation exists, but an `IntersectorBrep` adapter for `topology::Brep` does
not. Both comments and the unit notes name this missing adapter. Wiring p5 does
not certify paths that still stop earlier at construction or intersection.
The runner now explains the analytic revolution base of its sphere fixture,
and its README identifies the existing working `[0, 0, 1, 1]` sphere UV domain.

Current native result: **773 cases, 689 exact passes, zero failures, 75 deferred,
9 rejected**. Deferrals: `boolean.topology_intersector_p2` 10 (missing B-rep
adapter), `topology.brep_p2` 1 (`MakeShellVertexTopology`, now reached through
p7/p9 with a real infinite region), `topology.brep_p3` 5, `topology.brep_p8` 59.
The former p9 case now reaches p2 rather than an invalid region lookup. All
689 perturbations are detected. No tolerance or absolute floor changed.

### boolean.merge_p3 (#245): orchestration coverage and dependency boundaries

All five functions are ported: `PiecewiseMerge`, `merge_breps`, `BooleanLists`,
`BooleanTrees`, and `NonManifoldBoolean`. The code after each missing dependency
call remains present. The engine/storage adapters delegate to the canonical
constructor, intersector and manifold-boolean owners instead of duplicating their
claimed implementations. No geometric-success parity is claimed: list preparation
currently stops at `topology.brep_p3`'s copy constructor (or its empty constructor
for surface inputs), binary trees/folds at `boolean.merge_p2`'s constructor, piecewise
cases at `boolean.topology_intersector_p2`'s `IntersectInsertRelate`, and the isolated
nonmanifold wrapper at `boolean.merge_p4`'s `ManifoldBoolean`.

Passing tree paths reorder/transfer stable input handles and append unused
operands. At a77d8005 the runner echoed the C++ fixture graph by handle; those
103 cases tested Rust slot identity/order/null state, not independent Rust graph
serialization. The Opus review correctly identified that the earlier graph-parity
wording overstated this coverage.

The corrected runner imports only the initial native fixtures, keeps actual Rust
B-reps in its pool, and reads every returned Rust B-rep through the existing Rust
stitch digest. The C++ side walks its result after the operation; its fresh export
is independently imported and projected to the same digest. Compared fields are
identity/order/null slots, exact face/edge/vertex counts, and ordered vertex
positions. No complete graph payload is echoed as Rust output. Adjacency, edge
curves, face surfaces, geometry tolerances and attributes are not compared. This
is a deliberately limited result projection, not complete graph parity. Real boxes reproduce the input geometry of
`TestSmPiecewiseMerge`, `TestSmBooleanLists`, and `TestMergeBrepsOperation`; the latter
three paths remain deferred. `TestSmMaterialCensus` is not reproduced end-to-end.
PiecewiseMerge is ported generically over `PiecewiseAccess`; no kernel
implementation exists yet (it belongs with the SmMerge engine, merge_p2/p4).
The fixture Pool is not that engine.
The current harness supplies no surface-list inputs or SSI skip relations, so no
parity is claimed for planar extraction or SURFACEMAP branches. This is a coverage
limit, not a passing branch.

The kernel tree API uses `Vec<Option<Operand>>`, preserving consumed input slots
on failure and defined null outputs of repeated leaves. The wired
`api_brep::boolean_trees` now uses `Vec<Option<Brep>>` for the same lossless
behavior. Its FFI wrapper reuses the merge_p3 guard, accepting safe repeated
leaves and refusing binary null dereferences rather than classifying all repeated
leaves as undefined behavior.
The `BooleanLists` connected-component loop retains the mark lock through copy and
removal, bounds iterations by the initial face count, and errors if removal makes
no progress. UB-188 records the native tree-index/null-dereference guard (source
inspection only, not a reproduced crash).

Source audit WR-196 also found that `BooleanLists` uses `new SmSurface(*source)` for
both input and output surface copies, rather than the virtual `Copy`: C++ slices
the derived geometry. The current Rust `SurfaceKind` has no base-only variant.
That specific missing constructor branch is `UNPORTED(surface.surface_p1)`; the
port must not substitute a derived-preserving `Surface::copy` and silently fix
the native behavior. `surface.surface_p1` remains partial; its notes record the
missing base-copy branch. No crash or geometric result is claimed for it.

Dependency-tag audit at reviewed f0957fc5 (`todo` owners may have claimed PRs;
none is reimplemented here). Batch #259 updates are recorded below:

| Tag | Upstream callee | Owner status / action |
| --- | --- | --- |
| `boolean.merge_p2` | `SmMerge::SmMerge`, wire-processing environment | todo; constructor and canonical engine environment needed by the already-ported merge_p5 call; no second mate map is fabricated |
| `boolean.merge_p4` | `SmMerge::ManifoldBoolean` | todo; argument-complete engine adapter |
| `topology.brep_p3` | `SmBrep` empty/copy constructors, `SmBrep::MergeBrep` | todo (#204); allocation context, both empty-constructor defaults, null stack context, copy maps/flags retained |
| `topology.brep_p8` | `SmBrep::CreateFaceFromSurface` | todo; surface and UV domain retained |
| `surface.surface_p1` | sliced-base `SmSurface` copy constructor | partial; exact missing branch described above |
| `topology.brep_p5` | `SmBrep::StitchAndOrient`, `SmBrep::RemoveFaces` | todo; default arguments retained |
| `topology.brep_p4` | `SmBrep::ShrinkGeometry`, `SmBrep::~SmBrep` | todo; destruction owner checked in units.json |
| `primitive.primitive_creation_p1` | `SmPrimitiveCreation::Boolean2D` | todo; operands and operation retained |
| `boolean.topology_intersector_p2` | `SmTopologyIntersector::IntersectInsertRelate`, `RegisterExistingSSI` | todo; five SSI arrays retained explicitly, including empty-array calls |
| `topology.brep_p2` | `SmBrep::IsManifoldSolid`, `SmBrep::Notify`, `SmBrep::SetTolerance` | todo; API serializer and Pool notification/tolerance adapters retain arguments |
| `boolean.merge_p3` (removed) | `SmMerge::merge_breps`, `BooleanLists`, `BooleanTrees` | all three API back-edges wired; none remain in kernel code |

The Pool now delegates subset/control/SSI storage to the canonical
`TopologyIntersector`, SURFACEMAP lookup/users to `AttributeStore`, and mark
release to `MarkLock::release`.
Its imported surface owners preserve the native owned/unowned flag, with existing
legacy importer keys translated to canonical face arena identities. Without this
translation GetSurfaces missed the real box planes; the new adapter test caught
that latent mismatch and now asserts six box surfaces and zero shell-vertex
surfaces. A small public `clear_relationships` accessor
clears the intersector's existing private mate maps; it does not introduce a
second map or implementation. Existing SSI curve arrays are empty in this
adapter; a nonempty curve-storage path still explicitly defers to its unported
registration integration. Wire processing itself is already ported in merge_p5;
its canonical Relations/VertexCopy engine environment is missing in this Pool,
so that hook truthfully defers to merge_p2's engine integration, not to merge_p5.
Constructor, tolerance, notification, registration and destruction hooks now
return owner-tagged Unported, rather than a fixture-domain InvalidInput. The
new-face constructor hook retains SetTolerance(TRUE, TRUE), editing and append
tails after the constructor stub. The destructor hook retains its success tail: retire the old handle and clear
both other-operand slots. A null other skips destruction; Unported leaves the
operand live. B-rep handles are append-only, with a retirement set checked by
surface/result accessors, so reinstalled stale handles are rejected. The public
trait documents that contract.
Upstream's statusless mark destruction ignores ordinary release failures while
still propagating Unported; it uses the shared error::ignored helper.

All five families serialize actual Rust results on success. BooleanLists emits
its B-rep count, per-result digests and surface count; NonManifoldBoolean emits
its restored manifold flag, lamina mode and optional result; PiecewiseMerge
emits its live controls and optional result. Four serializer/adapter regression
tests demonstrate that Rust vertex changes, control changes and surface counts
change outputs, and exercise canonical subsets/attributes/marks and owner labels.
These are harness regressions, not geometric Boolean-success comparisons.
BooleanListsResult lives in merge_p3 and is re-exported at its existing API path.
The older unrelated include-file formatting commit is reversed, leaving those
three files at main's formatting; only this unit's four include files are checked.

The shim uses the persistent API fixture context for all operands, copies and
merge engines. Failed consuming native calls do not reveal a complete live-object
set, so their unknown objects are retained rather than risking double deletion;
a per-call stack context would leave their context/cache references dangling.
This fixture lifetime correction is not classified as an upstream defect.

The generic fold adapter checks canonical operand identity before draining any
input, matching the native duplicate-pointer validation. Owned `Brep` values
cannot alias; storage-handle drivers implement `BooleanDriver::same_operand`.
The regression verifies that duplicate handles leave the whole input unchanged.

The initial native baseline hit UB-190 in the two piecewise box fixtures with
imprinting and no keep-other flag. The overseer's independent diagnostic found
that the imprint early return deletes the other B-rep even for PARTIAL_MERGE;
PiecewiseMerge then reuses it for the next surface. The persistent-context
lifetime correction does not change this upstream defect. FFI and shim now
refuse that effective option combination only when the other operand is a box
with multiple surfaces. A shell-vertex other operand has no surfaces and never
enters the per-surface loop, so imprinting without keep-other remains accepted
for it, even when the primary is a box.
The two refusal fixtures compare only the FFI guard (both-errored/no_values),
not a native result or a dependency deferral. Their keep-other variants remain
native comparisons, as do all other accepted cases. Generated piecewise inputs
use shell vertices and retain all option masks, including imprinting without
keep-other. Four explicit point/box-primary fixtures cover that accepted path.

Current Opus-fix default comparison: 354 cases, 182 matched, 0 failed, 172 deferred,
`max_rel_err=0`, no absolute floors. The 126 matches with values comprise 103
leaf-only identity/order/null and B-rep-digest cases, plus 23 empty forests. Of the 56 both-errored cases,
54 compare native error statuses and two check only the UB-190 guard (180
operation comparisons + 2 guard self-checks, not 182 independent kernel comparisons). There are
zero geometric-success comparisons. Perturbation detected all 182 changed
outputs. Guard fuzzing passed 2,000 mutations: 1,292 rejected, 708 ran, zero
differences, crashes, panics or timeouts.

The wired API unit has 287 cases: 151 matching integer status/list records,
zero failures and 136 deferrals (merge_p2 114, brep_p3 21, brep_p2 1).
All seven existing cases that flipped from deferred to matching were audited:
`lists_empty`, `trees_not_binary`, and generated 0003/0025/0122/0208/0228.
They reach only the native empty-first-list or stack-underflow errors, before
missing geometry code. The new empty forest and unreachable-leaf cases match;
the defined null-leaf result defers at the existing IsManifoldSolid serializer.
API perturbation detected 151/151, and 2,000 guard mutations passed with zero
differences, crashes, panics or timeouts (325 rejected, 1,122 malformed, 553 ran).

Valgrind executed 50 programs, ten per family, through both native and Rust
runners. Its baseline output matched exactly and Memcheck reported zero errors
without suppressions. Leak checking was disabled: unknown live objects after a
failed consuming native call are retained, as described above. This does not
claim that those failure paths are leak-free.

Reverified after integrating topology.brep_p10 (#130), main through 0cf3e290:
workspace/CPP compile checks, clippy x2, workspace fmt and explicit own include
rustfmt, 1,402 kernel tests, four serializer/adapter tests, two merge_p3 FFI guard tests
and wasm all passed. Native comparison, perturbation, 2,000-case guard fuzz runs
for p3/API and the 50-program Valgrind sample passed on this corrected tree.
The touched intersector_p1 passed its default run: 827 cases, 690 matched,
zero failed, 137 deferred, with_values=687/no_values=3. All p3 case outcomes and
first-blocker labels are unchanged from a77d8005; compared payloads now contain
actual Rust digest data instead of C++ graph echoes. The full 213-unit matrix
passed: 114 PASS / 99 PARTIAL, zero failures. Its per-log audit checked all exit
codes and count categories (including rejected fixtures), with zero green units
carrying deferrals. Previous 212/211-unit runs are historical evidence only.

Latent shim identity limitation (Opus re-check f0957fc5, source inspection only):
`harness/cpp/shim/boolean_merge_p3.cpp::put()` resolves a result identity by
pointer lookup in `originals`. Once merge_p2/p4 permit consuming Boolean success
paths, a freed operand address could be reused for a result allocation and alias
that result to an original identity. Track operand liveness or null deleted
entries before enabling those paths. They remain deferred today; this is a
harness limitation, not an upstream defect or an observed comparison failure.
No shim lifetime change is made here.

After batch #259 / main 7374781c5, the surface-slicing note is WR-196 (the
shared UB/WR number space had a collision). `CopyFaces` now requires a real
`NotifyEnv`: `boolean_lists_with_notifications` forwards caller-owned notification
state, while the existing arena-only entry retains its signature and explicitly
defers at the missing `topology.brep_p2` notification boundary if reached. The
component-copy tail still calls the canonical `copy_faces` with `use_merge=true`.
No temporary records/attributes are fabricated. The now-landed brep_p5
`StitchAndOrient` and `RemoveFaces` calls are wired, retaining all input options
and the actual editing flag. The fixture Pool still lacks the SmMerge engine's
concrete `IntersectorBrep` binding; the topology_intersector_p2 adapter deferrals
remain integration limitations, not a claim that its generic algorithms are
absent from main. No intersector or merge engine is reimplemented here.

Final ready verification after a clean merge of origin/main 154853c7 (including
46ddc4ff): workspace fmt and explicit own include rustfmt, workspace/CPP compile
checks, both clippy configurations, 1,410 kernel tests, four serializer/adapter
tests, two merge_p3 FFI guard tests plus the separate API guard test, and wasm
all passed on sc-builder-13. The merge_p3 and api_brep default difftests retained
the counts above with zero failures; their pulled reports are byte-identical to
the reviewed reports. Full-matrix, perturbation, fuzz and Valgrind evidence above
applies to f0957fc5. Log: `status/merge_p3-ready-gates.log` in the swarm directory.

Review 183c96b7 constructor correction: BooleanLists surface-list preparation
and disconnected-component extraction now call an argument-complete
`UNPORTED(topology.brep_p3): SmBrep::SmBrep` adapter, with both model-size
defaults and the null stack context explicit. `Brep::new()` cannot substitute
for the native constructor: it lacks the void infinite region and context
tolerance initialization. All surface-copy/face-construction and component
copy/stitch/remove tails remain after this call. No #204 constructor is co-ported.

Opus re-review follow-up: should-fixes are implemented in 5c66d017, 4eff8875
and 13b83371. The shim still couples PiecewiseMerge use-existing/create-new flags
to CookieCutter/Imprinting bits. Imprinting without create-new is not expressible.
Generated cases remain shell-vertex only, without repeated leaves or multi-operator
trees; fixed fixtures exercise the existing repeated-leaf/error and real-box paths.

Update (PR #287, #243 follow-up): the isolated nonmanifold wrapper's boundary
named above has moved. `DependencyMerge::manifold_boolean` now runs merge_p4's
`ManifoldBoolean` on an adopted engine (`DependencyMerge::with_engine`), and without
one defers to `boolean.merge_p2`'s `SmMerge::SmMerge`; the runner's pool driver is
retagged the same way. The 40 nonmanifold deferrals therefore moved from
`boolean.merge_p4` to `boolean.merge_p2`; none of those newly compares. New fixture family 6
(`fixtures/boolean.merge_p3/engine-ner.json`, 33 cases) runs the adopted-engine path
against a context-only native `SmMerge` with the same explicit members. With at most
one operand it covers only `ManifoldBoolean`'s NER checks, the restored manifold flag
and the lamina mode the engine received, not geometry. The historical audit table
above records the boundary at the time of #245.
