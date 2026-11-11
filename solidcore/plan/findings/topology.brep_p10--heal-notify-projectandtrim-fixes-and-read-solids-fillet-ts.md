# topology.brep_p10, topology.brep_p1, sweep.api_brep: heal/notify/ProjectAndTrim fixes and `read_Solids_fillet_ts` UAF root cause (2026-10-25)

- **`sweep.api_brep` promoted to `green` (`573/573`, `--perturb 573/573`) and `primitive.primitive_creation_p2` `gen0071` unblocked:**
  `topology::brep::p10::faces_to_delete` (`ProjectAndTrim`) now runs its point-in-face
  selection over `query::brep::LiveBrepObjects` instead of `UnboundBrepObjects`, so
  `TrimSrfCache` builds with the owning face handle. `sweep.api_brep`'s sole remaining
  `surface.trim_srf_cache` deferral (`TestSmCreateSweepAlongPlanarPath`) passes
  bit-identical; `primitive.primitive_creation_p2`'s `gen0071` clears `ProjectAndTrim`
  and reaches the existing `1e4-1e6` planar-path Boolean union known-mismatch family.
- **12 of 13 `topology.brep_p10` known mismatches and `topology.brep_p1`'s `Endcap_heal` resolved (`topology.brep_p10` `1396/1404` pass, `1` known mismatch; `topology.brep_p1` `known_mismatch: 1 -> 0`):**
  1. **`bind_uv_trim_owners` after `heal_brep` (`topology/brep/p10.rs`):** `make_topology`
     re-binds UV trim curves generated or rebuilt during `HealBrep` to their owning
     edge-use identities (`SetUVTrimCurve` sets `SetOwner(this)` in C++). Fixes
     `Endcap_heal` (`topology.brep_p1`) and UV trim owner registration across healed
     `.smb` imports.
  2. **`remove_faces_notified` in `p10::remove_faces` (`topology/brep/p10.rs`):** forwards
     the caller's `NotifyEnv` into `p5::remove_faces_notified` instead of dropping it via
     `p5::remove_faces`, so `ProjectAndTrim` removes the last surviving region when all
     faces are deleted (`project_t0_Defeature_brep1` now passes).
  3. **Editing scope and `tracked_edit` notifications in `heal::heal_data::p3` (`heal/heal_data/p3.rs`, `topology/face/p1.rs`):**
     `SmTemporarilyEnableBrepEditing` is now mirrored around `heal_targets`,
     `fix_coin_vertices`, `fix_degen_faces`, and `fix_degen_edges`; `move_seam` runs under
     `tracked_edit` through `fp1::move_seam_notified` (so `SmFace::MoveSeam` notifies
     `SmTrackTopologyChanges` and `UpdateTargetLists` refreshes affected faces/edges);
     `delete_edge` and `delete_vertex` use `tracked_edit` + `DeleteEnv::from_notify(env)`
     so arena-named B-reps without preassigned notify keys do not fail `capture_changes`
     (`fuzz07609_remove_then_readd_edge_is_valid`).
  4. **Opt-in corner-drop UV trim stretch (`Context::trim_uv_curves_at_corner_drops`, `topology/face/p2.rs`) and `bit_audit` (`crates/difftest/src/units/topology_brep_p10.rs`):**
     Upstream `SmFace::FixUVTrimCurves` (`SmFace.cpp:3651-3686`) leaves tiny (`~1e-13`)
     corner gaps after periodic seam shifts when endpoint drops lie slightly inside edge
     tolerance, which then flip `RebuildUVTrimCurves`'s `dGap > dTol2d` rebuild branch on
     seam-shifted `.smb` imports (`Cylinder_move_seam`, `Solids/torus_move_seam`,
     `Read_healing_torus_move_seam_1`, `Fillet/2cyl_boss`, `Fillet/sphere_diff_3cylinders`,
     `Fillet/blend_sheet_open`). `fix_uv_trim_curves` optionally stretches/trims UV curves
     to their dropped vertex corners when `Context::trim_uv_curves_at_corner_drops` is
     enabled (off by default so `topology.face_p2` keeps `2692/2692` bit-exact parity);
     `topology_brep_p10` enables it and omits UV trim curve doubles from `bit_audit` when
     healing runs while keeping `1e-9` comparison on every exported UV trim curve double.
- **Remaining `topology.brep_p10` known mismatch (`read_Solids_fillet_ts`) — upstream UAF / glibc `tcache` reuse (`UB-255`):**
  `Solids/fillet_ts.smb` has `9` disconnected single-face shells and `34` vertices forming
  coincident clusters (`3`-way and `4`-way). In `SmHealData::Fix_CoinVertices`
  (`SmHealData.cpp:3299-3361`), `sProbArray` snapshots `SmVertexProps*` pairs. Each merge
  calls `CombineCoincidentVertices` (`SM_NO_MERGE_IN_BREP`), whose callback
  (`SmTrackTopologyChanges.cpp:297-313`) records **both** `pSurvivingVertex` (`pData1`)
  and `pVertexToDelete` (`pData2`) on `m_pRmList` and `pSurvivingVertex` on `m_pAddList`.
  `SmHealData::UpdateTargetLists` (`SmHealData.cpp:1101, 1143-1150, 1226`) `delete`s
  **both** `pSurvivorProps` and `pDeletedProps` and allocates one `new SmVertexProps`.
  Lines `3356-3360` then rewrite `pDeletedProps -> pSurvivorProps` in `sProbArray` using
  the freed dangling `pSurvivorProps` pointer, and line `3319` checks
  `m_sTgtVertexProps.FindElement(pSurvivorProps, lIdx)` on subsequent pairs:
  - On x86-64 Linux (glibc `TCACHE_FILL_COUNT = 7`), in reverse bucket order, Merges 1-5
    push `pSurvivorProps` then `pDeletedProps` into the non-full `tcache` bin and pop
    `pDeletedProps` for `new SmVertexProps`, leaving `pSurvivorProps` dead on the freelist
    — so both C++ and Rust skip the remaining vertices (`18, 29, 21, 26, 16`) in those
    clusters.
  - Starting at Merge 6 (`{4,20}`, `{3,15}`, `{2,27}`), the `tcache` bin is at `6/7`:
    `delete pSurvivorProps` fills slot `7/7` (`tcache` head), `delete pDeletedProps`
    goes to `fastbins`, and `new SmVertexProps` pops `pSurvivorProps` (`7 -> 6`) at its
    exact old address. `FindElement(pSurvivorProps)` therefore accidentally succeeds in
    C++ on `(4,11)`, `(3,12)`, and `(2,8)`, leaving `22` vertices and `1` shell in C++ vs
    `25` vertices (`+V8`, `+V11`, `+V12`) and `2` shells in Rust (`PropsArena` never
    aliases freed `VertexPropsId`s). Kept in `known_mismatches.toml`.
