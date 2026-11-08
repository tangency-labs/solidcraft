# topology.brep_p6: the `MakeFaceTopology` remainder (2026-10-21, no disagreements)

- **What was ported.** `SmBrep.cpp:14141-14828` in full. That covers the face and face
  uses, loops and loop uses, the mark-locked edge-loop block (keeper shell, `RemoveShell`
  plus region `Remove` plus `delete` of the other loops' shells, wire edge-use reuse,
  new edge-use/vertex-use pairs, `sUsedEdges` / `bVerifyRadialSectors` radial-sector
  recovery with a temporary UV trim curve, `AddOrientedEUPair`, CW/CCW links), loop
  vertices (shell-vertex reuse and shell deletion, `TransferChildrenTo` of a vertex's
  shell), `cpOptUVCurves` and `CreateUVTrimCurves`, `TestClosureSplitRegion` and the
  final `SM_NO_ADD_TO_BREP`. The former `UNPORTED(topology.face_p3): SmFace::SetTolerance`
  stop was not a missing `face_p3` call but this unported remainder. It is gone.
  Every callee is in a green or partial unit.
- **API.** `make_face_topology` / `make_face_topology_attempt` take `&mut Kernel` (the mark
  lock), the caller's `Marks` and a `NotifyEnv`. `topology.brep_p7`'s `make_face` gained a
  `NotifyEnv` argument. `make_face`, `make_face_with_curves`, `copy_faces`,
  `trimming_tools::trim_surface_with_model_space_curves` and
  `FilletBrep::make_face_brep` now take `&mut Kernel`. Their library callers already
  held one; only tests and runners changed.
- **Coverage (op 3).** `fixtures/topology.brep_p6/make_face.json` (89 cases, see its
  README) plus generated cases build kind-5 wire networks. Kind 5 is a new shared
  fixture block: `MakeShellVertex` points joined by `MakeEdgeTopology` line edges. The
  cases run sequences of `MakeFaceTopology` calls and compare every call's status and
  outputs, the context mark counters and locks, and the whole post-operation graph:
  surfaces, UV trim and edge curves sampled, tolerances, every link. They pass with 0
  failures, including holes, loop vertices, shared edges, open boxes of 1 to 5 faces,
  a non-manifold book, B-spline patches and tiny, huge or off-plane inputs.
- **Deferred.** After the merge with `topology.face_p4` (#239), every `bCalculateTrims`
  case and every radial-sector recovery defers at `SmSurface::DropCurve`
  (`intersect.surface_drop_lib_p3`), inside `face_p3`'s `CreateUVTrimCurves` or
  `edgeuse_p3`'s `CreateUVTrimCurve`. Given UV curves on a plane defer at `SmCrvOnSurf`
  (`curve.crv_on_surf`). A face that closes a shell (the sixth cube face, zero-length
  loops) defers at `SmBrep::CalculateBoundingBox` (`query.brep`) in
  `TestClosureSplitRegion`. Of the 89 fixture cases, 50 pass and 39 defer.
- **Not compared: edge marks.** `TransferChildrenTo`'s nested `GetWireEdges` (inside
  `RemoveShell` and the loop-vertex shell merge) marks the C++ edges. `topology.shell`
  keeps those marks in a per-call record (its documented deviation), so the edges'
  mark flags after the operation differ (seen on holes and wire loop vertices). The
  export therefore carries no edge marks. The context's mark counters and locks, which
  count every claim, are compared.
- **C++ guard.** The shim refuses (`-2`, rejected) a call whose input edge use has no
  edge, mate, shell or vertex, whose loop counts exceed the edge uses, or whose
  non-shell loop vertices would be merged into the NULL keeper shell of a face without
  edge loops. Upstream dereferences those. The port returns `InvalidInput` there.
- **Downstream.** `topology.brep_p7` 316 cases: 238 pass, 78 deferred, 0 failed (was 237
  and 79). Its face paths now get past `MakeFaceTopology`, and 18 of them reach
  `SmFace::GetTolerance` through a face-owned surface (`topology.face_p2` dispatch
  stubs in `surface`). `boolean.merge_p2`'s `MakeFaceuse` now completes its face: 45
  cases, 32 pass, 13 deferred (was 30 and 15). Its `MergeContext` gained `SplitMut`
  (a mutable primary kernel for the face's mark lock), and `MakeFace` notifies through
  the copy's face, edge and entity identities, so that its `Notify(SM_NO_COPY)` of the
  new face finds the record. Before that fix, gen0006 and gen0008 failed with
  "notified object is not a topology object". `primitive.api_primitives`: 1445 pass,
  536 deferred, 0 failed, unchanged (its face paths go through the unported
  `topology.brep_p8` and `primitive_creation_p*`).
