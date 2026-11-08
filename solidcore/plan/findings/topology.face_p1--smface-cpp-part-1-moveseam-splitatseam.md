# topology.face_p1 (`SmFace.cpp` part 1: `MoveSeam`, `SplitAtSeam`, `CreateOrientedTrimBoundary`, `CalculateUVDomainFromUVTrimCurves`, ...; PR #148, 2026-10-16; partial, no disagreements)

- **Mostly deferred by design.** Every mutating member starts with `SmFace::Notify`
  (`topology.face_p3`), `CalculateUVDomainFromUVTrimCurves` makes the UV trim curves
  first (`CreateUVTrimCurves`, `topology.face_p3`), `CreateOrientedTrimBoundary` evaluates
  edge uses (`NormalizedEvaluate`, `topology.edgeuse_p3`), and the seam members walk the
  upward loop uses (`GetUpwardLoopuses`, `topology.face_p4`). The passing cases are the
  early returns, error paths and guards (faces without surface / loops, quiet seam
  classifications, non-periodic surfaces, loops of another face). When those units land,
  their porters wire the stubs here and the deferred cases become comparisons of the
  full records (orientation, UV trim curves, domains) the runner already emits.
- **Seam inputs.** `HasSeamProblem` is unported, so the Rust side of the seam ops asks
  the C++ shim for its classifications (`Probe` op) and rebuilds classifications of the
  same shape; nothing past the first unported callee reads more than that.
- **C++ crash guard.** Upstream `SplitAtSeam`'s U-then-V branch recurses on the wrong
  face (`plan/upstream-issues.md` UB-137) and crashes in `MergeInterval`. The shim skips
  (`-3`) every seam call that can reach that branch: every `MoveSeam` with split past its
  early return (it recomputes the seam problem before splitting) and every `SplitAtSeam`
  merging in both directions on a face periodic in either (by the face properties' closed
  flags or by the surface itself). The Rust side reports those cases as deferred to
  `cpp-crash-guard:UB-137`; they never become comparable. **The guard is very
  conservative:** a guard-free sweep of 60,600 seam calls (15 files x faces 0-3 x rebuild x
  face properties x center x split, pre-review of PR #148) crashed 36 times, all
  `vase_rev.smb` face 0 `MoveSeam` with split and fabricated crossed-BOTH verdicts, all
  inside the guard; of the 7,056 `MoveSeam`+split calls past the early return the guard
  skips, 7,020 (about 99.5%) are valid. No input predicate separates the crashes (`cone` and
  `sphere`, also periodic in U only, do not crash with the same verdicts). It costs nothing
  today (the Rust side defers those calls at `GetUpwardLoopuses`, `topology.face_p4`), but
  **when face_p4 lands the guard must be replaced**, e.g. by running the C++ seam ops
  out-of-process or behind a fork-based crash trap, not by a looser predicate.
- **`MergeIntervals` wired (maintainer, after `intersect.curve_class_p5` landed;
  resolver by the porter).** `SplitAtSeam` calls the real
  `CurveClassification::merge_intervals` (vertices then edges, upstream's default; its
  status is ignored as upstream, `Unported` propagates). Its `MergeInterval` topology
  edits still defer to `intersect.curve_class_p2`, so it makes no faces or edges yet.
  The new faces, edges and vertices map to ids through the caller's `resolve`; a handle
  that does not resolve (to an entity of the right kind) is a loud `InvalidInput`, never
  an empty result, so a caller whose resolver is incomplete (`trimming_tools`'
  `SeamObject` has only loops and vertices; the runner resolves nothing) fails visibly
  once `MergeInterval` edits topology. No difftest case reaches it (the `face_p4` stubs
  and the UB-137 guard come first). The UB-137 guard stays: once `MergeInterval` edits
  topology, the C++ side still crashes there.
- **`IsCrossedByEdge` / `HasNearMisses` wired** (`intersect.curve_class_p7`, PR #65).
  `MoveSeam` without face properties calls them (`HasNearMisses` with upstream's default
  `bInSurfaceGaps = FALSE`, so it takes classification geometry: `move_seam` gained a
  `geometry: &impl ClassGeometry<O>` argument). Upstream returns plain booleans, so a
  failure reads as `FALSE`, except `Unported` and an unresolvable handle
  (`InvalidInput`). No difftest case reaches them: `GetUpwardLoopuses`
  (`topology.face_p4`) comes first.
- **Not compared.** The shim gives faces and edges plain B-spline copies of analytic
  geometry (what the Rust import rebuilds), so `MoveSeam`'s analytic branch
  (`CopyAndAddAnalytics` of the joined surface) and `SwapUV`'s conic UV trim curve
  mirrors are never compared with the C++.
- **Copies of the B-rep.** `face::p2::points_in_face` and `mass::integrator` hold the
  B-rep shared and run the port on a whole-B-rep clone per call (O(faces * B-rep size)
  over a sweep); passing the B-rep mutably removes that once their callers can.
- **Back-edges.** Wired: `trimming_tools` (`FlipFaceOrientation`; `SplitAtSeam` behind the
  `topology.face_p4` `HasSeamProblem` stub), `edge` (`CalculateUVDomainFromUVTrimCurves`).
  `face::p2::points_in_face` and `mass::integrator` hold the B-rep shared, so they run the
  port on a copy (the UV trim curves it makes, a cache upstream fills from `const`
  methods, are not kept). Not wired: `surface::p3`'s `RemoveUVTrimCurves` of the owner face
  (a surface cannot reach its owner's B-rep there) and `proto_topology::p2`'s
  `CalculateUVDomainFromUVTrimCurves` (a `ProtoBrep` face handle cannot reach a `Brep`
  face); both keep their `UNPORTED(topology.face_p1)` markers with that note.
- **Open follow-up for this unit (found by the #147 round-6 and #156 round-4 reviews):**
  upstream `SmFace::CalculateUVDomainFromUVTrimCurves` intersects the face domain with
  the surface domain *before* `CreateUVTrimCurves`, ignoring the status
  (`sThisDomain.Intersect(sSurfDomain, rNewFaceDomain)`), so when `CreateUVTrimCurves`
  fails it returns face ∩ surface (or the `Init()`ed extent when they are disjoint); the
  port (`topology/face/p1.rs`) returns the raw face domain on that path. Latent while
  every caller defers at `CreateUVTrimCurves` (`topology.face_p3`; 38 cases in
  `offset.offset_geometry_creation`); fix it in this unit, not in its callers.
- **`heal.heal_data_p3` back-edges wired** (after #164): `Fix_MoveSeam` and
  `Fix_SplitFaceAtSeams` call `move_seam` / `split_at_seam` with upstream's arguments (the
  face properties' classifications, seam problems and the properties). Classification
  handles have no B-rep meaning there, so the resolver is `&|_| None`: a merge that makes
  topology is `InvalidInput`. **Caveat:** the healer ignores both statuses as upstream
  (`ignore_soft`), so that `InvalidInput` is swallowed there and the split reports no new
  faces. It is unreachable today (`MergeInterval` makes no topology until
  `intersect.curve_class_p2`); that unit's porter must give the healer a typed resolver.
- **`CalculateUVDomainFromUVTrimCurves` early clip restored** (follow-up to #148, found
  by the #146 review): upstream first sets the output to
  `sThisDomain.Intersect(sSurfDomain, rNewFaceDomain)` (status ignored; disjoint domains
  give an `Init()`-ed extent, `SmExtent2d.cpp:406-428`) and returns it as `SM_SUCCESS`
  when `CreateUVTrimCurves` fails (`SmFace.cpp:1301-1315`). The port had dropped it as
  "unused" and returned the unclipped face domain on that path; it now computes it. No
  case reaches the path: `CreateUVTrimCurves` is an `UNPORTED(topology.face_p3)` stub
  that always defers. **For the topology.face_p3 porter:** once it is wired, a failed
  `CreateUVTrimCurves` must still return this clipped domain (callers in
  `boolean.topology_intersector_p1` keep it on success).
