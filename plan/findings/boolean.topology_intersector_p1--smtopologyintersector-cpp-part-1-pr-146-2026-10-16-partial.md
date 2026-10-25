# boolean.topology_intersector_p1 (`SmTopologyIntersector.cpp` part 1, PR #146, 2026-10-16; partial, no disagreements)

Builds on #121 (`topology.proto_topology_p3`) and #150 (`intersect.curve_class_p5`), both
merged as-is. After the maintainer review and the swarm pre-review (follow-ups below): 827
cases (8 upstream-derived fixtures, 19 edge cases built in code, 800 generated two-B-rep
scripts): 653 pass bit for bit (`max_rel_err 0`), 0 fail, 174 deferred, 3 cases where both
sides reject the script (cut at op 0); `--perturb` SENSITIVE 653/653. (Before the reviews:
819 cases, 679 "passes" of which 211 compared nothing, 140 deferred.)

- **Scripts on two real B-reps.** The shim builds the primary and the other `SmBrep` with
  the `sc_topology_fixture.h` blocks (`.smb` files, a wire edge, a face from a surface, the
  `my_test_topology` network), makes an `SmTopologyIntersector` over them and runs a
  script of the unit's functions; the Rust side imports both exports into two `Brep`s
  behind one `IntersectorBrep` adapter. The file-static helpers (`sm_CanImprint`,
  `sm_FindFirstLowestDimensionSol`, `sm_IntersectShellVertices`,
  `sm_IntersectWireLaminaEdges`, `sm_SetEdgeClass`, `sm_CheckInteriorEdge`,
  `sm_TestAndSplitTwoEdges`, `sm_TestAndImprintFace`) are verbatim copies in the shim.
- **`Relate(pBrepTopo, NULL)` stores the null mate.** `m_pBToO->Insert(obj, NULL)` and
  `m_pOToB->Insert(NULL, obj)` succeed for a face (vertices and edges fail `NER` first), so
  `GetBrepMate(NULL)` afterwards returns the face and a second `Relate(face2, NULL)` is
  `SM_ERR` (`pBrp != pBrepTopo`). The port's maps are `BTreeMap<O, Option<O>>` /
  `BTreeMap<Option<O>, O>` for that reason (caught by the diff: 2 generated cases).
- **Hash-order outputs** (plan hazard `ptr-hash`): `GetMappedBrepObjects` lists the
  values of `m_pOToB` and `GetSubset` the keys of `m_pSubset` in `std::unordered_map`
  order upstream; the port lists them in handle order and the shim sorts upstream's lists
  by export handle before comparing (`plan/upstream-issues.md` ND-2). Their consumers
  (`SmMerge`) are not ported yet.
- **`CheckIfHaveExistingSSI` re-copies the accumulated lists.** For every matching table
  entry the function appends that entry's arrays and then copies *every* curve of the
  accumulated 3D/UV lists again (`for jj in 0..r3DCurves.GetSize()`), so with two matching
  entries the first entry's copies are copied once more and the originals of those copies
  leak. Ported as is (the port's copies go to the handle store; the earlier handles stay
  unreferenced). A 3D curve that is not an `SmBSplineCurve` is `SM_ERR` after the arrays
  were partly filled; the port reports the `SM_ERR` in `ExistingSsi::status` with the
  outputs filled so far, as upstream leaves them.
- **`UpdateObjectTolerance`** (`SmTol`, left unported by `geom.tol`) is co-ported as
  `geom::tol::update_object_tolerance` on a concrete `Brep`; its face branch stubs
  `SmFace::SetTolerance` (`topology.face_p3`) and `SmFace::GetEdges` (`topology.face_p4`).
  The intersector reaches it through `IntersectorBrep::update_object_tolerance`. Its
  "nothing to do" status `SM_ERR_INVALID_INPUT` is `SolidError::Upstream { code: 1007 }`
  (added as `error::SM_ERR_INVALID_INPUT`); callers ignore it, as upstream.
- The runner builds `SmCurveClassification(curve, ivl, NULL, tol)` from the port's setters
  (including #150's `CurveClassification::set_interval`) as `ReSet` does
  (curve, interval, source zone tolerance `SM_UNINIT_TOL` when not positive, one interval
  over the range with the tolerance on its three classifications).
- **Deferred (174, after the reviews).** `SmFace::GetTolerance` (82, `topology.face_p2`):
  `SmSurface::DropPoint` of a face-owned surface takes `SmTol::GetZoneTol3d(surface)`,
  stubbed in `surface.surface_p5`; so every `sm_TestAndImprintFace` / `VertexFaceIntersect`
  / `FaceFaceCoincidence` drop on a face defers (a `Surface` cannot reach its face's
  tolerance without the B-rep). `SmBrep::GetVertices` / `GetEdges` / `GetTolerance` (74,
  `topology.brep_p1`, open PR #145): the common edge/vertex lists, the shell-vertex and
  wire/lamina intersections (no passing case for those two), and a manager made with a
  zero approximation tolerance. `SmFace::GetVertices` / `GetEdges` (11,
  `topology.face_p4`, open PR #134): `FaceFaceIntersect` and `FaceFaceCoincidence` with two
  faces on an edge. `SmFace::CalculateBoundingBox` (7, `query.face_p1`): `EdgeFaceIntersect`
  with an unconnected face.
- **Compared green paths:** the mate maps (`Relate` with its tolerance maximisation and
  error statuses, `GetOtherMate` / `GetThisMate` / `GetBrepMate` / `IsMatedPair`,
  `GetMappedBrepObjects`), `SetSubset` / `GetSubset`, both `Fixup*PointClassMates`, the
  getters/setters of both constructors, the intersection table (`RegisterExistingSSI` from
  `_p2` on the C++ side, the port's table filled directly; `CheckIfHaveExistingSSI` with
  copies, the opposing-null entry, `ClearExistingSSI`), `sm_CanImprint`,
  `sm_FindFirstLowestDimensionSol`, `sm_SetEdgeClass` (trimmed and whole intervals,
  opposite primary edge uses, drops onto the edge), `sm_CheckInteriorEdge` (parameter and 3D
  tests, outside the interval), `sm_TestAndSplitTwoEdges` up to its split (no, far and
  end-point intersections), `sm_TestAndImprintFace` on planar faces (the plane's fast drop; other surfaces defer on the face tolerance),
  `VertexFaceIntersect` with null entries and empty lists, `EdgeFaceIntersect` for connected
  faces and empty lists, `FaceFaceIntersect` / `FaceFaceCoincidence` with empty or
  single-face lists, `ResolveIntersections` / `ImprintIntersections` on an empty manager
  (including the editing / "doing Boolean" flag changes).
- **Not in this unit:** `RegisterExistingSSI`, `IntersectInsertRelate`, the constructors
  (`boolean.topology_intersector_p2`; the two constructors are CO-PORTED here as `new` /
  `with_context`), `GetMappedOtherObjects`, `CoincidenceNotify`, `AreIntersectionLoopsClosed`,
  `RemoveRelationship`, the `Draw` / `Dump` utilities.
- **Back-edges.** `SmTopologyIntersector::GetOtherMate` in `topology::proto_topology::p3`
  (`ti_get_other_mate`, PR #121) is wired: `glue_coincident_brep_vertices` takes the
  lookup as a closure and the intersector passes its mate map (see the follow-up below;
  `RemoveRelationship`, `_p2`, will need a mutable mates trait instead of the closure).

### PR #146 maintainer review follow-up

Changes made for bherbruck's review (all re-verified; numbers below are the new run):

- **FFI guards.** `SetBreps` is tracked: `GetCommonEdges` / `GetCommonVertices`,
  `FaceFaceIntersect`, `FaceFaceCoincidence`, `ResolveIntersections` and
  `ImprintIntersections` are refused while either of the intersector's B-reps is null
  (they dereference them unchecked; crashes confirmed by the reviewer). `Relate` no
  longer needs a primary B-rep (upstream never reads `m_pBrep` there), only a non-null
  object and the maps of the two-B-rep constructor. Negative tolerances and inverted
  classification intervals are accepted (`SmExtent1d(t0, t1)` swaps). The shim no
  longer dereferences edge curves or face surfaces captured before a mutating op
  (`Two::put` looks them up by address in maps built at walk time). The ops that edit
  topology emit their outputs only on a success status on both sides.
- **Real coverage.** A script now runs up to its first unrunnable op on both sides
  (the record ends with `-4 index`), the fixtures and built-in cases put each mutating
  op last (one per case), and the generator only draws kinds that exist (no faces on
  wire / network B-reps, registered curves only after a `register_ssi`). `Relate`
  outputs the two objects' tolerances. `cases_both_errored` is now 0 (was 211 of 679
  passes); `PARTIAL boolean.topology_intersector_p1 cases=827 passed=642 failed=0 deferred=185 max_rel_err=0e0 blocked_by=query.face_p1(5),topology.brep_p1(79),topology.face_p2(89),topology.face_p4(12)`, `SENSITIVE boolean.topology_intersector_p1 perturbed=642 detected=642 unperturbed=0 deferred=185`. Ops with zero passing cases: `shell_verts`
  and `wire_lamina` (every case reaches `SmBrep::GetEdges` / `GetVertices`, `topology.brep_p1`).
- **Port fixes.** `update_classifications_after_split` classifies with
  `m_dThisApproxTol3d` (was `0.0`); `face_face_intersect` takes its face lists by
  reference and never writes them back; the statuses upstream ignores go through
  `ignore_soft` (`DropPoint` of `FaceFaceCoincidence` with `bOk = FALSE`,
  `dGap = SM_BIG_DOUBLE` on failure; the bounding boxes, `GlobalCurveIntersect`,
  `GlueVertices`, `CoincidenceCheck`, `FaceIntersect`, `Point3DClassify`,
  `MakeVertexLoop`, `MakeEdgeInFace`, `GlueEdgesGeneral`, `DeleteFace`,
  `SetTolerance` in `Relate`, `GetMaxGap3d` and the nested `UpdateObjectTolerance`
  calls in `geom::tol`), and the `unwrap_or` sites that swallowed `Unported` /
  `InvalidInput` are `ignore_soft(..)?.unwrap_or(..)`; `VertexFaceIntersect`
  re-reads the vertex tolerance for every face; `sm_IntersectWireLaminaEdges` takes the
  intersector's `GetThisApproxTol3d()` for `FindOrCreateProtoEdge`;
  this unit uses #150's `CurveClassification::set_interval`
  (`m_vInterval.SetMinMax`, status ignored by the runner); the imprinted interval ends
  are written back before the split updates (no stale copy after them);
  `FaceFaceIntersect`'s working face lists are bounded (`MAX_FACE_LIST`, a documented
  deviation); the stubs carry upstream's constant arguments (`NULL`, `SM_OT_SAME`,
  `0.0`, `FALSE`, `(TRUE, TRUE)`); a vertex without B-rep in `VertexFaceIntersect` is
  `InvalidInput`; a null start vertex in `sm_IntersectWireLaminaEdges` continues as
  upstream (`GetOtherVertex(NULL)`).
- **Back-edges wired after the merge of `main`.** `SmSurface::GlobalCurveIntersect` /
  `CoincidenceCheck` (`surface.surface_p8`, now partial) and
  `SmCurveClassification::Homogenize` (`intersect.curve_class_p6`, now partial) are
  real calls; the deferrals they caused now come from their own unported callees.
- **`intersect.curve_class_p5` (#150, merged).** `InsertTopologyIntersections` is a
  real call now. The `UNPORTED(boolean.topology_intersector_p1)` marker #150 left in
  `IntersectVertices` is not a mate lookup this unit can supply: it guards part
  5's own unported SSS block (`SmCurveClass.cpp:14826-15095`, knot snapping, surface
  solves, through-point edits), whose mate lookups come last; it is labelled as part 5's
  own deferral (`intersect.curve_class_p5`, the unit that owns the missing code). The block
  is the `GWC:CHANGED_SSS` branch of `SmCurveClassification::IntersectVertices`
  (`SmCurveClass.cpp:14643`, owned by `intersect.curve_class_p5` in `units.json`), not of
  `SetPointClassParameters` (`:5706`, part 1, ported). No difftest case of this unit or of
  part 5 reaches the block (neither `blocked_by` lists it), so no count changes. Porting
  it needs mutable curve / B-rep access in `IntersectVertices` (an API change to part 5)
  plus a mate-lookup closure; this unit's `other_mate` / `brep_mate` can be passed as that
  closure whenever part 5's owner ports the block.
- **Back-edge wired.** `proto_topology::p3::glue_coincident_brep_vertices` takes the
  intersector's `GetOtherMate` as a closure (`ti_get_other_mate` stub removed); the
  intersector passes its mate map, the proto-topology runner passes "no mates" (its
  shim's intersector relates nothing).

### PR #146 pre-review follow-up (swarm reviewer, 49b32145)

- `make_edge_for_interval`: an interval end that is not a vertex after the refresh no
  longer aborts; `MakeEdgeInFace` fails its `NER(cpVertex)` upstream with the status
  ignored and null outputs, so the port returns null edge and face after the curve copy
  (leaked upstream, kept in the store here). `imprint_interval_end`: `FindObjPoint3d`'s
  status is ignored (the point stays `UNDEF`); one null vertex out of two is now
  `InvalidInput` (`GlueVertices(NULL, v)` dereferences it). `edge_face_intersect`: the
  face-point evaluation goes through `ignore_soft` (the last swallowed-status site).
- `CheckIfHaveExistingSSI` keeps its partly filled outputs with the `SM_ERR` of a
  non-B-spline 3D curve (`ExistingSsi::status`), as upstream's only caller
  (`IntersectTopology`, `_p2`) reads `rbHaveExistingSSI` and ignores the status.
- `FaceIntersect`'s `pbOptCoincidenceFlag` is an input (skips its own
  `CoincidenceCheck`): the stub takes `Option<bool>`, fed from the coincidence block.
- `VertexFaceIntersect` with `bIgnoreProblems` skips a vertex without B-rep instead of
  failing. `GetOtherVertex(NULL)` is documented as null with `SE(SM_ERR)`.
- Minor: the edge curve of `EdgeFaceIntersect` and the classification curve of the
  split update are dereferenced where upstream does (after the filters, inside the
  loop); a null B-rep of the second edge in `sm_TestAndSplitTwoEdges` is `InvalidInput`;
  classification lists of different sizes are `InvalidInput` (upstream indexes out of
  bounds); the constructor stub carries `cpUVCurve = NULL`; the duplicated
  `sm_CheckInteriorEdge` call is gone; `geom::tol` applies the `SmZoneTol3d(double)`
  conversion (`SM_USE_DEFAULT` to the system tolerance) at its setters.
- Not changed, recorded: the shim does not re-walk the graph after a mutating op (a new
  entity reusing a freed address would be reported under the dead index on the C++ side
  only, a spurious fail, never a pass; re-walking would renumber survivors differently
  from the Rust arenas); `imprint_intersections` returns the deleted curve handles only
  with success (on failure the handles stay unreferenced in the owner's store, so nothing
  dangles); `RemoveRelationship` through the mate closure is `_p2`'s API change (noted in
  its plan notes).
- **#144 (`topology.proto_topology_p1`, merged after the pre-review).** It moved the
  B-rep walks of `ProtoBrep` into a `ProtoTopology` supertrait (`vertex_point` now
  returns `Result`, `edge_other_vertex` takes `Option<O>`, `edge_brep` returns the
  B-rep's `ObjectKey`). This unit's `IntersectorBrep` dropped its duplicates
  (`edge_start_vertex`, `edge_other_vertex_of`; the latter's null-vertex semantics are
  `edge::other_vertex`'s: `None`, as `SmEdge::GetOtherVertex(NULL)` after `SE(SM_ERR)`),
  renamed its handle-returning `edge_brep` to `edge_brep_handle`, and its runner's
  `Two` implements `ProtoTopology<H>` (the walks over the handles' own operand). No
  numeric change: the difftest result is identical. Likewise #151 (`intersect.curve_class_p9`,
  merged after it) extended `query::face::p2::CurveOnClassifyObjects` (edge, vertexuse,
  edgeuse, edge-curve and trim-curve identities, `face_surface` for `surface`, `curve`
  removed): the runner's `Objs` implements the new form with two Rust-side handle kinds
  (vertex use 12, trim curve 13; the shim never exports them) and `ClassGeometry::curve`
  resolves a trim curve through `edgeuse::p1::uv_trim_curve`. Difftest result identical.
- **Wired calls audited against their callees (brief lesson 21).** Each stub this unit
  wired to a ported function was checked for the exact upstream call: arguments, branch
  and the callee's Deviations line. `SmSurface::GlobalCurveIntersect` (`EdgeFaceIntersect`,
  five arguments, `surface::surface::p8::Surface::global_curve_intersect` with the handles
  in its documented order curve/surface; p8 builds the surface cache per call where
  upstream shares one, its own documented deviation; its unported callees return
  `Unported`). `SmSurface::CoincidenceCheck` (`FaceFaceIntersect`: the angle tolerance as
  the 3D tolerance, `bAllowExtensions = FALSE` as `whole_surface = false`, `pSCover`
  unused; plane override and `base_coincidence_check` defer through `Unported`).
  `SmCurveClassification::InsertTopologyIntersections(sols, 0)` and `Homogenize(other)`
  (`IIREdgeBrep`): part 5 rejects only `object_index > 1` and defers its edge branch and
  SSS block, part 6 defers its p2/p4/p7 callees, all as `Unported`. The intersector's
  `geometry` is part 5's `ClassGeometry`: the runner's `Two` answers
  `classification_vertex_point` and `topology_zone_tol_3d` (vertex, edge, face) as the
  `query.face_p2` runner does and leaves `vertex_face_gap` at the default (`None`: part
  5's adapter gap on main). None of these calls is reached by a passing case yet
  (`EdgeFaceIntersect` defers earlier at `SmFace::CalculateBoundingBox`, `IIREdgeBrep` at
  `SmBrep::GetEdges`), so they are wired, not certified. `SmTopologyIntersector::Relate`
  for #163's vertices: `SM_USE_OLDTOL` is defined (`SmConfig.h:227`), so the tolerance
  maximisation is live; `SetTolerance`'s defaults are `(TRUE, TRUE)`; `MergeVertex`'s
  own `SM_OLDTOL_LINE SetTolerance` after `Relate` is #163's `merge_vertex`.
- **#163 (`boolean.merge_p1`, merged after the pre-review).** Its co-port of the mate
  storage over typed ids (`MateMap`, `Relations`, at this unit's module path
  `boolean::topology_intersector::p1`) is kept in this unit's `p1.rs` (add/add merge; the
  intersector itself stays generic over handles). Its `UNPORTED(boolean.topology_intersector_p1)`
  stub `relate_new_vertex` is wired to the new `relate_vertices` (the vertex case of
  `Relate` over typed ids: conflicting-mate checks, the tolerance maximisation of both
  vertices, the map insertion); `merge_vertex` therefore takes the other B-rep mutably
  (`SM_USE_OLDTOL` raises the source vertex's tolerance too). Its runner already held the
  other B-rep mutably, so no runner change; `boolean.merge_p5` (merged from `main` after the
  wiring) passes it on from `process_wires_and_shell_vertices`, whose `other` parameter and
  its test/runner call sites therefore became `&mut Brep` in this PR (upstream mutates its
  non-const `m_pOther` member). Difftest re-runs: `boolean.merge_p1` PARTIAL cases=248 passed=153 failed=0 deferred=95 (unchanged from #163); `boolean.merge_p5` PARTIAL cases=140 passed=108 failed=0 deferred=32 (unchanged from #161).
- Coverage: a script cut at its first op compares nothing, so the runner reports it as
  an error on both sides and `cases_both_errored` counts it (the real-pass count of the
  PR body is passed minus both-errored); a few passes compare only a `-2` guard marker
  (`Relate` of a null). `SetBreps` is generated now (4% of ops), so the null-B-rep guard
  is exercised by the difftest. Final numbers: `PARTIAL boolean.topology_intersector_p1 cases=827 passed=653 failed=0 deferred=174 max_rel_err=0e0 blocked_by=query.face_p1(7),topology.brep_p1(74),topology.face_p2(82),topology.face_p4(11)`, `SENSITIVE boolean.topology_intersector_p1 perturbed=653 detected=653 unperturbed=0 deferred=174`, `cases_both_errored` 3 (scripts cut at op 0), 32 of the 653 passes compare one refused op's status only; real-value comparisons 618.


**Wiring follow-up: `SmFace::CalculateBoundingBox` through the adapter (2026-10-20).**
The 7 generated cases deferred under `query.face_p1` (`gen0011`, `gen0105`, `gen0124`,
`gen0133`, `gen0180`, `gen0513`, `gen0642`) stopped at the intersector's own private
`face_bounding_box` stub, a dispatch gap: `query::face::p1::calculate_bounding_box` is
ported (main since #238's batch) but takes the face's `Brep` and `FaceId`, which the opaque
handle does not carry. The call is now `IntersectorBrep::face_calculate_bounding_box(k, face,
tight)`, the last method of the trait (the pattern of the edge box; named after the upstream
call in full because open PR #229's `MergeBrep` already has a `face_bounding_box`), implemented by the difftest
adapter and by `boolean.merge_p4`'s `ArenaPair` on the B-rep behind the handle. The
intersector's one call (the edge-face box check, `bTight = FALSE`) takes the port's
non-tight branch, which is complete; the tight branch (`query.face_p3`) is never requested.
Case diff against main: 830 cases (with #272's regression fixtures since the merge of main
bb137d7d; 827 before), 713 pass, 0 fail, 117 deferred on both sides (693 / 137 before #149
wired `topology.brep_p2`'s stubs, merged at e5f3e441); the 7 now
run their face box and defer deeper, 6 at `SmFace::GetUpwardLoopuses` (`topology.face_p4`)
and 1 at `SmBrep::CalculateBoundingBox` (`query.brep`); no new compared values yet, so the
box itself is exercised but not compared until those stubs are wired. Perturb 690/690,
`FUZZ-PASS` 2000; `boolean.merge_p4` unchanged (311 cases, 30 pass, 278 deferred).
