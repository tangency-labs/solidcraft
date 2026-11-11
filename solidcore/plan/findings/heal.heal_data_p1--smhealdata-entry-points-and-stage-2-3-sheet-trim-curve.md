# heal.heal_data_p1 (`SmHealData` entry points and stage 2/3, sheet, trim curve, infinite region steps; `SmApiHealBrep`; PR #109, 2026-10-16; partial, no disagreements)

- **Built on PR #128 (heal.heal_data_p2).** The branch merges #128 (which merges #122,
  heal.face_props). The p1 steps run as codes `23..=31` of p2's heal scripts (shim
  `p1_step` in `harness/cpp/shim/heal_heal_data_p2.cpp`, `check_heal`, the p2 runner's
  `Step`), so the healer state compared after them is p2's full dump plus a stage 2/3
  face props snapshot (step 31). `SmApiHealBrep` has its own shim
  (`heal_heal_data_p1.cpp`). Merge #122 and #128 first.
- **Coverage is limited by stubs in other units, not by this unit.** Most deferrals
  (219 of 331 at merge time) are `SmFace::GetTolerance` (topology.face_p2): `Cache_FaceProps_Stage2`
  on a plane face, or a B-spline face whose surface queries take the zone tolerance
  of the surface's face (`surface::surface::p1::zone_tol_3d`, `surface::plane::p2`
  `zone_tolerance`). face_p2 is `partial` and its `tolerance` is ported, but those
  stubs only have the surface, not the B-rep, so they cannot be wired without a
  surface-to-B-rep accessor; whoever adds one should rewire them. Closed surfaces
  stop at `SmFace::HasSeamProblem` (topology.face_p4), stage 3 past its op gate at
  `SmFace::ClassifyLoops` (query.face_p4). Compared for real: `ReSet`,
  `MoveToNotYetProblems`, stage 2 on non-plane open faces (surf_extrude, surf_rev,
  surf_sphere150 and generated faces), stage 2/3 below their op gates (the
  `SM_ERR` paths, where stage 3 lists every face because its counts are still
  `SM_UNDEF_ULONG`), `Fix_BadSheets` on sheet faces (lamina closure traversals),
  `Make_UVTrimCurves(FALSE)`, `HealBrep(NULL)`, `SmApiHealBrep(NULL)`.
- **Props without a face.** Stage 2/3 and `Make_UVTrimCurves(TRUE)` on props that
  `Cache_FaceProps_Gaps` never set (no upstream caller does this) run `SetProps` on a
  null face. Upstream either fails the stage (op before `Fix_TolSizes`, a defined path
  through `SmTol::GetZoneTol3d(NULL)`) or dereferences it. The port returns
  `InvalidInput` for both and the shim refuses both (`-2`): this one defined path is
  refused too, deliberately, because the Rust `SmFaceProps::SetProps` takes a face id.
- **Last-step-only steps.** `ReSet`, `Fix_InfiniteRegion` and `HealBrep` on a B-rep
  free or rebuild the props the shim's registry tracks by address, so the FFI accepts
  them only as the last step (as `FreeTargets`).
- **aarch64 hosts: 2 expected failures (2026-10-17).** `gen0086` and `gen0248` differ in
  the edge-length histogram, as 26 `heal.heal_data_p2` cases do (UB-194, see General).
  Both pass with aarch64 casts in the port.
