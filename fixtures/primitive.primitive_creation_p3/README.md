# Primitive creation p3 comparisons

`cases.json` calls the pinned native implementation, including the actual
file-local `sm_CreateRuledSurfaceFromOffsetCurve` (the shim includes the source
with renamed externally visible symbols). The generated cases use fixed harness
seeds and scales from 1e-6 through 1e6.

| Path | Evidence compared on both sides |
| --- | --- |
| Ordinary ruled surface, surface-intersection attribute transfer | 25 surface samples, three input curve states, status and flags |
| Corner to pivot, including rational weight matching | Resulting surface, offset degree/control count/rationality and samples; original generator states |
| Circular arc before/past the apex | Surface samples plus offset/generator mutations; past-apex offset is intentionally not translated |
| Missing map | Error status and all three curves after degree reduction |
| Already-used generator, pre-existing missing-face state | Error status, marriage flags, and transformed offset before the early return |
| Sliver rejection | Error and mutated inputs, with below/equal/above length-threshold fixtures |
| Invalid capped repetition counts, empty curve sweep, null blend edge uses | Entry-point status and edit/context/output-list state; these are early exits, not geometry coverage |
| Swung primitive with no profiles | Successful empty-loop return and unchanged state, not a constructed surface |

The construction fixtures run through the production `PrimitiveModel`, using the
same new B-rep and primitive options as the native probe. In addition to the
original six fields (status, editing/Boolean flags and three output-list lengths),
they compare owning-list face/edge/vertex counts. Successful calls also compare
vertex positions, each edge's actual interval and five curve evaluations, and each
face's UV domain and nine surface evaluations. Evaluation statuses are retained;
failed operations compare partial topology counts without evaluating unfinished
geometry. Neither side substitutes cached B-splines for virtual geometry dispatch.

The current 296 cases yield 295 passes, no deferrals or failures, and the unchanged
UB-78 rejection. All 295 comparable native/Rust output digests agree exactly.
Nine former topology-construction deferrals now compare actual nonempty geometry,
including the existing zero and
negative tolerance cases. The repeated linear wire has two faces, seven edges and
six vertices; skin, curve-sweep and swung fixtures each have one face, four edges
and four vertices. The partial rotational sweep also builds one face, four edges
and four vertices. A native-enabled regression requires those counts, successful
construction and the complete geometry record on both sides; the repeated linear
fixture also exercises the public FFI output-buffer retry.

The production deletion adapter uses the landed context-only intersector with the
model's real notification records and object keys, matching the fresh native
intersector inside the rotational caller. The revolution API extracts and detaches
the actual first surface under those same keys, matching `GetSurfaces` and
`SetOwner(NULL)`; no geometry copy or owner guess replaces it.

Curve-sweep scaling, profile orientation, face-based skinning, classification,
cap construction, stitching, map-copy tails and topology edits are implemented
but paths beyond the specifically compared fixtures remain **uncertified**. A kernel regression checks
that preparation preserves the input curves and that the inverse placements
recover the original path. Another checks the transform/marriage mutations on
the missing-face early return. Neither replaces native comparison of the tails.

The empty skin fixture is rejected before C++ under existing **UB-78**, not
counted as a parity pass. The offset helper preflights the selected three-point
circle solve (**UB-57**) and rejects non-finite apex placement. Failed native
curve construction or non-finite fixture samples are construction rejections.
The Rust port still checks these inputs safely. No comparison tolerance or
absolute floor is added.

The adjacent surface-of-revolution API uses the same real model and now compares
its detached output's owner state, UV domain and 25 virtual point evaluations.
Its native unit has 879 exact passes and three remaining downstream skin/cache
classification deferrals; those paths are separate from this unit's completed
construction fixtures.


Review regressions (`review.json`) allow finite zero/negative rule tolerances and
zero/negative curve-sweep approximation tolerance through the checked FFI. Four
helper cases compare real values on both sides; two curve-sweep cases deferred
at topology construction at that historical checkpoint; they now compare geometry. That earlier update has 293 cases: 284 pass, 8 deferred,
1 UB-78 rejection and 0 failures.

Swung scaling retains the original derived `Curve` for virtual point evaluation
and global property analysis. Two Rust regressions use imported analytic lines
with a cached NURB differing by two ULPs, checking profile evaluation without a
plane intersection and path evaluation after an intersection. Both distinguish
the correct analytic result from a sliced B-spline. They do not add a native
comparison of the deferred swung construction/topology tail.

Curve-sweep scale-reference dispatch is also covered by a Rust regression.
`create_curve_sweep` and `prepare_curve_sweep` accept the original `Curve` reference;
the sampled scale law uses its virtual interval, point solve and evaluation.
Path and scale-curve inputs remain B-spline copies, matching the explicit native
copy constructors. An imported analytic line with a two-ULP cached-NURB offset
produces an exact endpoint scale of 2; deliberately slicing that reference changes
the endpoint and fails the exact assertion. This exercises preparation through
its public API; it is not an additional native/topology comparison.

The native shim still builds B-spline lines/arcs. Real `SmLine`/`SmCircle` fixtures
must be added when the corresponding construction paths stop deferring, so these
derived dispatch branches can be compared through the native entry points too.

The swung builder also mirrors `CreateSwungSurface`'s
`GetOrCreateGwNurbPointer`: it obtains the section NURB before the trajectory
NURB and materializes only a missing analytic cache, using the caller's Kernel
and original owner. Existing cached NURBs are retained bit for bit. Explicit
B-spline copy-constructor paths in curve-sweep preparation are unchanged.
Two Rust regressions compare 25 surface samples for lazy profile/path/both
against eagerly built line NURBs and pin an existing two-ULP cache discrepancy.
These are builder/dispatch regressions, not native topology comparisons.

The third review update merges canonical main while retaining both sides of
`plan/findings.md`. `review_negative_tolerances.json` adds negative skin and swung
tolerances, including a swung call with no profiles whose tolerance is never read.
The FFI still validates finite values, record lengths, degrees and path counts.
The no-profile case compares the successful empty return; skin and nonempty swung
construction still defer at `topology.brep_p8`.
The current gate has 296 cases: 285 pass bit-identically, 10 defer at
`topology.brep_p8`, 1 is rejected under UB-78, and none fail.

Both stitch defaults now call the landed `Stitch::do_stitching` with the actual
Kernel and target B-rep. A shared `StitchCallback` qualifies entity ids with that
B-rep and forwards selection and map changes to `TopologySweep`. Linear caps use
fast edge comparison, no vertex-edge splits, non-manifold mode and region nesting,
with result validation disabled; the sweep default retains its native constructor
defaults and the caller's manifold flag. Rust regressions check map mutation,
nonzero owner ids, conflicting originals and restoration of editing/boolean flags
when both defaults reach the current `topology.brep_p2` notification dependency.
These do not certify successful native stitching or resolve the production-store
adapter tracked by #255. The curve-record format also still cannot express
reversed circular arcs (`end_angle < start_angle`).

The `topology.brep_p8` and `topology.brep_p3` face services of `PrimitiveAccess`
are shared trait defaults over the caller's real `SweepAccess::with_brep_mut`.
The differential entry points use `PrimitiveModel`; the small one-B-rep adapter
remains only in default-adapter tests. Production rotational cleanup calls
`KernelManifoldIntersector::delete_faces_with_keys` with the model's actual
notification records and identity callback.

Records retain status, editing/boolean state and output-array sizes, then compare
owning-list face/edge/vertex counts. Successful constructions also compare vertex
positions, actual edge intervals and five edge evaluations, and face UV domains
and nine surface evaluations. The native constructor uses virtual geometry
methods. Evaluation statuses remain visible in the records.

The gate has 296 cases: 295 pass, none defer, and one retains its UB-78 rejection.
The shared `axis_inside_face` default calls the existing SweepStore classifier;
neither production nor test adapters duplicate it. The merged defaults preserve
consumed curve slots instead of changing their container shape.
