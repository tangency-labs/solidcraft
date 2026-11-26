Updates: ND-491

#### ND-491 (update, 2026-10-30). `SmApiNonManifoldRotationalSweep` without a selection reaches it in `boolean.api_brep`
- **Trigger:** three `boolean.api_brep` fixtures in `fixtures/boolean.api_brep/smapi_more.json`
  (from `TestSmBrep.cpp` `TestNonManifoldSweepDistinctTarget`'s whole-B-rep passes): a
  `SmApiCreateBox(origin, 10, 10, 10)` solid swept by `SmApiNonManifoldRotationalSweep`
  90 degrees about the z axis through the origin, `bDoStitching` TRUE, `bDoMerge` FALSE, no
  face, edge or vertex selection, so the call reaches `SmTopologySweep::DoSweep`'s
  whole-B-rep branch with stitching, the trigger of the original record:
  - `rotational_sweep_whole_box` (`pRegionBrep` = the swept B-rep, #52),
  - `rotational_sweep_region_null_whole` (`pRegionBrep` NULL, #70),
  - `rotational_sweep_region_swept_whole` (`pRegionBrep` = the swept B-rep, #72).
- **Observed:** the C++ side returns `SM_ERR` (1001) in some runs and succeeds in others;
  when it succeeds its record (B-rep counts and face areas) equals the Rust one. Which of
  the three fail moves with the cases run before them: whole-unit runs failed {#70},
  {#70, #72} and {#52, #70}; a `--filter rotational_sweep` run failed {#72}; four
  `--filter rotational_sweep_region` runs passed all of them. `--heap-pads 1,3,8` and
  `--heap-pads 2,5,13` flag exactly these three cases, and no other `boolean.api_brep`
  case, as `unstable (in-process)` (the runner calls C++ in-process, so a rerun changes
  only the heap state and C++ state left by earlier calls).
- **Rust:** the port sweeps the silhouettes in ascending edge id order and always succeeds,
  with the result C++ gives when it succeeds.
- **Status:** the three cases are parked with `cpp_defect = "ND-491"` in
  `crates/difftest/known_mismatches/boolean.api_brep.toml` (glibc), like the
  `sweep.topology_sweep` whole-B-rep cases. Removing the markers runs them again.
