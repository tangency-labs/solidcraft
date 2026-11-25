# topology.brep_p7 (`SmBrep.cpp` part 7: `ReadFromFile`, `MakeFace`, `MakeFaceWithCurves`, `GlueEdgesGeneral`, `CopyFaces`, `MergeSelfIntersectionCurves`; PR #155, 2026-10-16; partial, no disagreements)

- **Mostly deferred by design.** Every path that builds topology reaches an unported
  `SmBrep` part first: `MakeTopologyFromData` (`topology.brep_p10`) for every read past
  `SmBrepData::ReadFromFile`, `MakeShellVertex` / `GetInfiniteRegion` (`topology.brep_p9`
  / `_p1`), or `SmFace::SetTolerance` (`topology.face_p3`) inside `topology.brep_p6`'s
  `MakeFaceTopology`, for every face made, `SmBrep::Notify`
  (`topology.brep_p2`) for every glue past its early returns, `SmBrep::SetTolerance`
  (`topology.brep_p2`) for every copy. The passing cases are 12 read errors of damaged
  streams the brep_data guard accepts (pinned by a deterministic search: an overwritten
  digit of an integer token), the face makers' input errors, `MakeFace`'s null-surface
  status, the glue no-ops (compared as the B-rep's counts after the op) and
  `MergeSelfIntersectionCurves` without curves. All 197 passes are no-ops, empty inputs or error statuses; none
  completes a path that builds topology: 110 glue no-ops (null, identical or wrapped-to-identical
  edges; compared as the B-rep's counts and sorted edge and vertex tolerances after the op), 64
  `MergeSelfIntersectionCurves` without curves and 23 statuses (`with_values=198`,
  `no_values=0`; after the merge with #154 the shared fixture registers every edge, wires included, so the generated glue `gen0014` on a wire network resolves to two distinct wire edges and now defers at `SmBrep::Notify`, as upstream reaches it first); the shim's `-2` guards are deferrals, not passes. Before the pre-review of
  PR #155 no `ReadFromFile` case compared anything.
- **C++ guards.** A damaged `.smb` stream goes through `topology.brep_data`'s stream guard
  on both sides (rejected streams are not generated). A damaged stream whose C++
  `SmBrepData` read still succeeds is not run in C++: `MakeTopologyFromData` uses its
  cross-reference indices unchecked (`plan/upstream-issues.md` UB-164). Both sides decide
  this with the C++ reader; the Rust side defers to `cpp-guard:unvalidated-topology`. The
  guard is needed: with it disabled, single-byte overwrites that `SmBrepData` accepts crashed C++ (SIGSEGV) in 11 of 600 streams (100 each of `Endcap`, `Mouse`, `box_with_large_fillet`, `cone`, `disk`, `ellips_ts2`), and a batch of 100 such streams per file crashed in 13 of the 15 files (pre-review of PR #155: 2 of 80 random streams). It also skips the safe streams, and no
  input predicate short of running the C++ separates them; it costs nothing today, since
  the Rust read defers at `MakeTopologyFromData` (`topology.brep_p10`) anyway. **Revisit
  when `topology.brep_p10` lands** (e.g. run the C++ read out of process). The built-in
  overwrites of the leading `//Brep Starts` comment were dropped: C++ reads them exactly as
  the plain file. The other shim guards answer `-2` (upstream would dereference null) and
  are deferred, not compared: `cpp-guard:ub-165` (`MakeFaceWithCurves`, below),
  `cpp-guard:null-curve` (`GlueEdgesGeneral` with `SM_OT_SAME`, equal intervals and one
  edge without curve, `CurveMaxDistanceBetween`; no fixture edge lacks a curve), `cpp-guard:null-surface` (`MergeSelfIntersectionCurves` on a
  face without surface). `GlueEdgesGeneral` of distinct edges sharing both vertices runs in
  C++ (the pre-review of PR #155 ran all 36 such calls of the fixture files to status 0); the
  Rust side defers those at `SmBrep::Notify` (`topology.brep_p2`).
- **Gaps in other units' ports:** the port of `SmCurve::DeformToEndPointTargets`
  (`curve.curve_p6`, green; its own "NOT PORTED" note) has no `pOptSurfCrvOnSurf`.
  `MakeFaceWithCurves` with `pbOptCrvOnSurf` passes the surface there, and for a curve on
  surface with a gap target upstream edits its UV curve in place. The port defers exactly
  that call (`UNPORTED(curve.curve_p6)`, a curve on surface with a target); other curve
  kinds dispatch before the argument is read upstream, and without targets upstream
  returns at once, so those call the port. One built-in case reaches the deferral
  (`face_curves_cos_open_loop`: three UV sides with `crv_on_surf` TRUE; C++ builds it,
  status 0 in the round-3 review run). That target-deformation gap remains recorded in
  `curve.curve_p6`'s notes. The old B-spline-only `CrvOnSurf` gate is removed:
  `MakeFaceWithCurves` now retains the actual surface with checked exact snapshots
  and native owning-copy semantics.
- **Resolved optional-surface endpoint gap (2026-10-22).**
  `Curve::deform_to_end_point_targets_on_surface` restores the optional surface and
  orientation branch; `MakeFaceWithCurves` passes its actual surface. It drops target
  points using the current UV endpoint guesses and edits the UV B-spline, retaining
  native default tangents and replacing derived UV curves when required. The original
  `face_curves_cos_open_loop` now compares 10 values with C++ (status 0, max relative
  error 0). Against exact-main `3e811d0c7`, this is the only existing-case outcome
  change; no cases were removed. Ten new scaled/oriented plane/B-spline fixtures add
  three comparisons and seven honest `SmSurface::DropCurve` deferrals. Native direct
  endpoint samples on B-spline and derived-line UV curves match the Rust regression
  for absent, same and opposite orientation. No tolerance or FFI guard was changed.
- **MakeFace with a square loop** (`kind 1`): the shim builds the loop with
  `MakeShellVertex`, `MakeWireEdgeVertex` and `MakeWireEdge`. The Rust side mirrors its
  `-3` when `CreateLineSegment` fails for the side, and otherwise defers at
  `MakeShellVertex` (`topology.brep_p9`; `MakeWireEdge*` are `topology.brep_p3`) until the
  same loop can be built in Rust.
- **Back-edge wired:** `trimming_tools::trim_surface_with_model_space_curves` calls
  `make_face_with_curves` (its new face owns the surface as `(SmFace_TYPE, 1 + index)`).
- **`topology.shell` wired** (after #154): `GlueEdgesGeneral`'s shell merge asks the ported
  `Shell::is_inner_shell` (`SmShell::IsInnerShell`, status ignored as upstream: only a
  successful TRUE swaps the shells). Its ray classification needs the kernel's mutable
  state, so `glue_edges_general` and `merge_self_intersection_curves` take `&mut Kernel`.
  Unreachable today: the shell merge comes after `SmBrep::Notify` (`topology.brep_p2`).
- **Back-edge wired: `boolean.topology_intersector_p1`** (#146). Its `GlueEdgesGeneral`
  stub over object handles became `IntersectorBrep::brep_glue_edges_general` (every
  upstream argument, `pInsideFaceuses` `NULL` explicit); the intersector runner's adapter
  calls `glue_edges_general` on the handle's B-rep, and `face_face_intersect` takes the
  kernel mutably. Recorded in COORD.md with the overlap of #145, #204 and #227 on the same
  trait.
- **UB-165.** `MakeFaceWithCurves` with `pbOptCrvOnSurf` TRUE, 3D curves and no UV curves
  dereferences the null UV array once `MakeFace` succeeds; the shim returns `-2` for such
  calls and the Rust side defers them (`cpp-guard:ub-165`; the port itself returns
  `InvalidInput`). Upstream's own input checks (`SmBrep.cpp:15722-15741`: no loops, a
  surface degenerate to a curve or a point) return before it and are run and compared.
- **Reviews of PR #155 (maintainer at a5359b48; cross-model and oursland at bbdb8bf1),
  all verified against upstream and fixed:**
  - The earlier review caught erasure of plane/surface-of-revolution kinds into their
    B-spline bases (`SmPlane::EvaluatePoint` and `SmSurfOfRevolution::EvaluateSTEP`
    are overrides). The concrete-Surface follow-up now preserves those kinds rather
    than deferring construction. The existing B-spline `face_curves_cos_open_loop`
    fixture remains unchanged and still exercises the `curve.curve_p6` target gap.
  - `MergeSelfIntersectionCurves` keeps upstream's loop: the classification is a stub
    (`intersect.curve_class_p4`), then a second classification of fewer or more than one
    interval is skipped before its merge, is `SM_ERR` after it, and one whose middle is
    off an edge is skipped (`SE(SM_ERR)`).
  - The `topology.edgeuse_p1`, `topology.edgeuse_p2`, `topology.loopuse` and
    `topology.vertex` runners (and `topology.edgeuse_p3`, which shares edgeuse_p2's
    `inexact_geometry`) deferred inexact `.smb` imports to `topology.brep_p7`
    `ReadFromFile`; the blocker is `MakeTopologyFromData` (`topology.brep_p10`), so they
    are relabelled.
  - The `SmBrep::Notify` and `MakeTopologyFromData` stubs now take upstream's arguments
    (the merged edges; `FALSE, TRUE, TRUE`).
  - The FFI ran the native read without the B-rep data check whenever a public flag said
    the bytes were a test file. It now decides this itself: only bytes equal to one of
    the 15 shipped ASCII test files (compiled in) skip the check.
  - `GlueEdgesGeneral` called `SetTolerance(r, FALSE, FALSE)`; upstream's
    `SetTolerance(dEnclosingRadius)` takes the defaults `TRUE, TRUE` (`SmEdge.h:431`),
    which also raise the vertex tolerances. Glue records now carry the sorted edge and
    vertex tolerances.
  - Two edges without curve are the same geometry upstream (`pSurvCurve == pDelCurve`,
    `NULL == NULL`, `SmBrep.cpp:9875-9879`); the port demanded both curves.
  - The runner decoded orientation code 0 (`SM_OT_UNKNOWN`) as `Opposite`; upstream
    tests `== SM_OT_SAME` / `== SM_OT_OPPOSITE`, so UNKNOWN takes neither branch. Codes
    0..2 decode exactly, others are refused, and the generator draws 0.
  - `CopyFaces` reports `SM_ERR` when a face fails (copies kept); the runner now
    records that status. `MakeFaceWithCurves` converts the caller's UV curves to 2D in
    place, so the later UV trim curves are copies of the converted ones (the port
    converted a clone). The FFI plane-normal check is upstream's precondition
    (`LengthSquared() > SM_EFF_ZERO_SQ`) and `MakeFace` kind 0 no longer checks the
    unused side. The `MergeIntervals` and `SmFace::Notify` stubs now take upstream's
    arguments (the face, its surface through it, the 3D curve; the notify operation
    and data).
  All but the read check and the orientation decoding are behind unported stubs today.
