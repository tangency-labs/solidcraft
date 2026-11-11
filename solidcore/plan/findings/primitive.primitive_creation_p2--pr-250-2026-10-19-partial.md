# primitive.primitive_creation_p2 (PR #250, 2026-10-19; partial)

All twelve owned functions of `SmPrimitiveCreation.cpp` are ported with their tails
after dependency calls: the partial cone, sphere and torus, the taper extrusion, the
curve sweeps from edges and faces, the pipe sweep, the static planar-path sweep and its
`SweepProfilesAlongPath`, the skin and `sm_CreateSkinPrimitiveInternal`, and the face
blend. `PrimitiveCreationAccess` extends p3's `PrimitiveAccess` with the face copy, the
offset-map attribute store and defaults that return `Unported` for B-rep services of
units not ported yet (tolerance, shrink, lamina test, face deletion, stitch,
edge/vertex cleanup, manifold test, boolean trees). `SmTrimType` and `ProjectAndTrim`
come from `topology.brep_p10` (#130); `project_and_trim` is a required access method,
like p3's `axis_inside_face`, because it needs the caller's `ProjectEnv` identities. The 8-argument `CreateSkinPrimitive` has no caller and is
out of scope. `USE_NEW_ANALYTICS` is active, so full turns go to p1's analytic
constructors. No recursion; loops are bounded by their input lists.

The native gate is 283 cases / 127 pass / 0 fail / 150 deferred / 6 rejected, maximum
relative error zero, no absolute floors. Deferrals: 56 at `SmBrep::SmBrep`
(`topology.brep_p3`: the scratch B-reps of skin, taper and planar path), 75 at
`topology.brep_p8` (planar faces of the profiles, `CreateFaceFromSurface` of the analytic
faces, `CreateFacesFromSurface` of the sweep faces) and 19 at `SmBrep::Notify`
(`topology.brep_p2`, inside the ported `SmBrep::Transform` of p3's curve sweep that the
pipe calls). The six rejections are
the guarded regression fixtures of `guarded.json` (UB-215, HANG-35), never passes.

Comparable cases are early exits: the cone's height, turn, zero-side and axis checks
with the editing flag they leave on, degenerate sphere/torus radii (line or circle
construction fails after edit mode is on), full turns with degenerate radii through p1,
skins of fewer than two profiles (cleared lists, sized and all-false ownership report),
curve sweeps of no edges or faces. Every case that builds topology defers: the scratch
B-reps of the skin, taper and planar path (`SmBrep::SmBrep`, `topology.brep_p3`), and
the sweep faces of the partial primitives and the pipe (`topology.brep_p8`). The upstream
skin, draft-sweep, taper and pipe tests run the real C++ and defer in Rust; they certify
nothing past those calls. Kernel unit tests check the entry checks on a B-rep with only
its infinite region (a removed check cannot hide behind a deferral).

Deviations, none compared yet:
- **Skin ownership report.** An open profile is passed to `build_composites_from_curves`
  as a whole, and every curve of it is marked transferred; upstream marks only the curves
  its composite builder consumed, which can differ for a lone curve shorter than the
  tolerance. Exact parity needs the composite builder to report its input indices
  (an option, not built).
- **Taper.** Analytic offsets stay B-splines (upstream casts `SmCurve*` to
  `SmBSplineCurve*` after `CreateTrimmedOffsets`); split curves are found among the
  segments by exact interval and ends; a single split curve gives an empty second
  ruled interval (`Extent1d::new()`) where upstream reads past its array (UB-112).
- **Upstream null dereferences** (a missing edge curve, a failed copy) are errors.

Native defects found while building the harness (see `plan/upstream-issues.md`):
- **UB-215:** `SweepEdgeHigh` uses an edge's curve after `MakeFaceWithCurves` freed it
  (partial cones with a profile line below the zone tolerance, partial tori with a small
  minor circle). The FFI rejects those, mirroring the cone's own early failures.
- **HANG-35:** closed shells a few zone tolerances across take minutes to place in their
  region. The FFI rejects spheres, tori and cones from `5e-6` to `1.5e-4` at any turn;
  the generator keeps skins and planar paths at `1e-3` or more.
- **WR-191:** a failed `SmMerge::ManifoldBoolean` leaves the context in boolean mode; it
  leaked from a failing planar-path case into the next three generated cases. The shim
  clears the flag before each case.
- The partial sphere's and torus's lack of a zero-turn check (only the cone has one) is
  upstream behaviour and is ported as is.

These were found with about 450 isolated native probes (one process each, 20-60 s
limits), backtraces from a scratch `SIGSEGV`/`SIGPROF` handler (no debugger here), and
a Python copy of the generator to recover the inputs of deferred cases.

Back-edges wired: six `sweep.api_brep` sites and the `sweep.api_surfaces` skin now name
the B-rep constructor they still need; the offset executive's pipe site now names
`SmBrep::CreateFacesFromSurface` (`topology.brep_p8`). No p2 stub remains.

Validation (after merging #233's head `c33cc6d3` and `origin/main` `fb58ea76`):
formatting, workspace and native-feature Clippy, 1,464 kernel tests, wasm32 and the
three native FFI guard tests pass. Perturbation detects all 127 comparable p2 cases. The
1,000-mutation campaign has 282 executed, 511 rejected and 207 malformed cases, with
zero differences, crashes, panics or timeouts. All six touched units pass with their
remaining deferrals: p1 689/75/9 rejected, p3 284/8/1 rejected, B-rep sweep API 306/267,
surface sweep API 831/51, offset executive p1 687/5/1 rejected, topology sweep 554/14
(pass/deferred). Nine planted faults (cone height, turn and edit flag; skin report,
profile count and output reset; pipe output reset; the full-turn window; the UB-215
guard) are each caught by the kernel tests; six also fail the difftest, and the other
three cannot show there (one turns compared cases into deferrals, two reset lists the
runner always passes empty). `topology.face_p3` and `topology.brep_p5` became partial on
main, so the lamina-edge test and the edge/vertex cleanup now call their ports.

The full `--green --no-report` gate completes with 226 units: 114 passed, 112 partial, 0 failed.
- **Co-port with `heal.stitch_p1` (#196).** p1 owns the `Stitch` struct, the callback and
  the `SM_API` entry points; p2 adds `impl Stitch` members in `heal::stitch::p2` and wired
  p1's three `UNPORTED(heal.stitch_p2)` API stubs (`SmApiStitchIntoSolid`,
  `SmApiStitchIntoShell`, `SmApiStitchAdvanced`). The harness is p2's own (same B-rep
  blocks and export as p1's, copied, so fixtures are interchangeable).
- **Mostly deferred by design.** `DoStitching` starts with `SmBrep::Notify`
  (`topology.brep_p2`), so every `DoStitching` case, every `StitchIntoShell` case with a
  positive tolerance and the three API entry points defer there; `StitchIntoSolid` defers
  at `SmBrep::CalculateBoundingBox` (`query.brep`). Neither has a passing case. When
  those land, `DoStitching` runs into `GlueVertices` / `FindAndSetInfiniteRegion`
  (`topology.brep_p5`), `SqueezeEdge` / `DeleteFace` (`topology.brep_p3`),
  `GlueEdgesGeneral` (`topology.brep_p7`), `MakeVertexSplitEdge` (`topology.brep_p9`)
  and `SmFace::IsLamina` / `IsDegenerate` / `Notify` (`topology.face_p3`); the runner
  already emits the full outputs and the B-rep digest after the op.
- **What the passes cover.** One-list `StitchVertexPairs`: the sorting, coincidence scan,
  gap bookkeeping, `SelectTopologyToKeep` and the manifold / squeeze skips (the glue is
  deferred). `SplitEdgesWithVertices`: the per-vertex `sm_TestAndSplitEdge` drops, far and
  close-to-boundary outcomes (an interior split defers at `MakeVertexSplitEdge`).
  `StitchIntoShell`: only the path where the doubling loop never runs (`max_tol <= 0`).
- **Guards.** `-3` (upstream never returns): `StitchIntoShell` when `max_tol / 100`
  underflows to zero (HANG-33, confirmed by a C++ run), `StitchIntoSolid` from a zero
  or negative B-rep tolerance (HANG-24; a non-finite diagonal ends or skips the loop).
  The FFI refuses a vertex repeated in the one-list `StitchVertexPairs` list (UB-178).
  Upstream defects kept by the port: WR-186 (`DoStitching`'s sticky candidate flags),
  WR-187 (a cancelled squeeze reported as done), UB-197 (stale edge after a failed glue with `m_bIgnoreProblems`).

### PR #250 review fixes (2026-10-20)

Review by bherbruck at `751f8b4d`; merged main `ed142cd9` and the stack's heads #213 `44942f7d` and
#233 `d05892fa` first.

**UB-50 through the taper.** `SmCurve::FixChainCurvesList` compares each candidate with the ends of
the chain built so far (start, end, the end link's mid point), not with one other curve, and every
deletion leaves a slot that is read again. Isolated native probes (one process each): two identical
lines, a line and its reverse, two identical arcs, and the lines `(0,0,0)-(3e-5,0,0)`,
`(3e-5,0,0)-(1,0,0)`, `(1,0,0)-(0,0,0)` (a repeat of no curve) segfault; with the first line
`5e-5` long it runs; squares with a repeated or reversed side run under each normal and cap. With a
supplied normal the taper may reverse all curves and chain again (`:3714`); a Python model of the
chaining and `ComputeCrvsNormal` found sets that delete only there, and six probed natively segfault
under the normal the curves wind against and run under the other one or a computed normal. The end
cap at the offset end (caps 2 or 3, `:4368`) chains the trimmed offsets through a copy of their list
(`:4382`), so a deleted curve is freed again at `:4477`: generated case 235 of the new taper
generator (a closed back-and-forth loop, side 100, -Z) aborted with `double free or corruption
(!prev)`, only with that cap. The FFI replays both input passes (the second whenever a normal is
supplied) and, with that cap, refuses a closed chain narrower than `2 * SM_ZONE_TOL_3D` (vector area
against length; wider than the crashes: a loop whose first line is 1 long runs). The offsets'
chaining before the caps (`:3884`) is not checked. An arc record's mid parameter is its mid angle
(within `1e-15`, measured with the Rust port).

**Stubs and numbers.** The sphere, cone and torus sites of `api_primitives` name their next unported
call (`SmBrep::SmBrep`, `topology.brep_p3`); the blend names `SmBrep::CreateFaceFromSurface`
(`topology.brep_p8`), the first unported call after the edge-use check and blend surface. The other
p1 and p3 stubs there belong to #213 and #233. The SweepEdgeHigh use after free is UB-215 (UB-202 went
to #168, UB-212 to UB-214 to open PRs #266 and #264). `ComputeCrvsNormal` failures leave a zero normal, as upstream.

**Validation.** p2 311 cases / 129 pass / 169 deferred (brep_p3 71, brep_p8 78, brep_p2 20) / 13
rejected / 0 fail; perturb 129/129; fuzz 1000 clean (536 rejected, 197 malformed, 267 ran);
api_primitives 1981 / 1445 / 536 deferred (brep_p3 282 that named p2); p1 773 / 689 / 75 / 9; p3 296 /
285 / 10 / 1; api_brep 573 / 306 / 267; api_surfaces 882 / 831 / 51; offset_executive_p1 693 / 687 / 5
/ 1; topology_sweep 568 / 554 / 14; 1,547 kernel tests, 4 FFI tests, wasm, both Clippy runs, fmt and
the two `include!`d files; green 237 units, 114 passed, 123 partial, 0 failed. Planted faults: 10 of
11 caught (unit tests, and the four guard faults also as native crashes of `guarded.json`); a
supplied-normal comparison against `UNDEF` instead of zero is unobservable today.


### PR #250 second review (2026-10-21)

Merged main `7b57d8de`, #233 `f810c60a` (its `p3/construct.rs` edge-blend call taken) and main
`ca3bbd2b`; the kernel test model and the difftest runner gained the three `SweepAccess` items #233
added. `SmBrep::SetTolerance` (upstream defaults: only if larger, cascading to the boundaries),
`SmBrep::IsManifoldSolid` and `SmMerge::BooleanTrees` (B-reps moved out, checked first, written back in
`sOrderedBreps`' order) are wired; a union of two B-reps defers at `SmMerge::SmMerge`
(`boolean.merge_p2`).

The taper guard now replays the reversed chaining only where upstream may take that pass, and its end-cap
rule refuses, where upstream may reverse, a closed chain whose short curve meets one over 100 times longer
at a corner sharper than `3e-5` radians. 7,236 native probes: 1,078 crash, the guard refuses all of them
(as the pushed head's did) and now refuses 1,805 that run instead of 5,558.

`--fuzz 1000` on the merged tree found UB-247 (`fuzz00880`): with the start cap,
`CreatePlanarFaceWith3DCurves` fails after building edges on the input curves when an end meets no other end
or more than one, and the taper deletes the curves those edges use. About 400 native probes: every crash had
a closed loop and such an end; curves closing no loop and clean loops (a self-crossing bow-tie, a square with a
repeated or reversed side) ran. The FFI refuses, with the start cap, curves that close a loop while an end
meets no other end or more than one within `2 * SM_ZONE_TOL_3D` (repeats of another curve left out). Five
crashing shapes are in `guarded.json`, four start-cap inputs that run in `review_duplicate_curves.json`; seven
planted faults in the guard are each caught by its test.

Gate at `1527b886`: p2 328 cases, 129 pass, 0 fail, 179 deferred (`topology.brep_p3` 81, `topology.brep_p8`
78, `topology.brep_p1` 20), 20 rejected; perturb 129/129; fuzz 1000 clean (560 rejected, 190 malformed, 250
ran, 0 differ); p1 773/689/75/9, p3 296/285/10/1, api_primitives 1981/1445/536, sweep api_brep 573/306/267,
api_surfaces 882/831/51, offset executive 1032/714/317/1, topology sweep 568/558/10; fmt, workspace and native
Clippy, 6 FFI tests, 187 difftest runner tests, 1,595 kernel tests, wasm32; `--green --jobs 4`: 242 units,
116 passed, 126 partial, 0 failed.

Main `a6132883` (#133's caller `Kernel` through curve and surface calls) does not compile with the stack yet:
45 call sites, 34 in the carried p1, p3 and topology_sweep code (#213, #233, #222) and 11 in p2. This head
stays on `ca3bbd2b`; p2's sites are fixed when #233, merged with main, is merged in again.
