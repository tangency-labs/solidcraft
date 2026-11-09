Updates: plan/upstream-issues/ND-1--srand-time-null-in-face-sampling-and-shell-classification.md

#### ND-1 (update, 2026-11-01). Confirmed on the C++ reference: the clock-seeded sampling reaches the stitch and changes a stitched solid's volume

ND-1 listed `SmFace::GetPointsInFace`'s `srand((unsigned)time(NULL))` (`source/SMLib/src/SmFace.cpp:21471`) as unconfirmed, and its trigger as "inner-shell tests on wire shells". It is confirmed here on a faceuse-shell path inside `SmStitch::StitchIntoSolid`. On a real body it decides the result.

- **Location (call path, from gdb):** `SmFace::GetPointsInFace` (`srand` at `SmFace.cpp:21471`) <- `SmTopologyTraverser::IsInnerShell` (`SmTopologyTraverser.cpp:1239`, which samples 3, 6 or 12 points per face) <- `SmBrep::TestClosureSplitRegion` (`SmBrep.cpp:28118`; its `IsInnerShell` calls at `:28270` and `:28274`) <- `SmBrep::GlueEdgesGeneral` <- `SmStitch::DoStitching` <- `SmStitch::StitchIntoSolid`. All 14 `srand` calls of the run come from this path.
- **What happens:** the stitched body has 5 regions and 8 shells. Whether a shell closed by a glue is inner or outer is decided by rays fired from the clock-seeded sample points. The verdict sets which regions are void, so the stitched solid's volume depends on the second the process runs in. Faces, edges, vertices, regions and shells, the manifold test and the area are the same either way.
- **Trigger:** STEP Tools corpus `ap203/bull.stp`, body 0, through the difftest STEP corpus path (`difftest --step-case`: `CreateFacesWithCurves` per face, `SmApiStitchIntoSolid`, then counts, manifold, area and volume at accuracy 1e-6, `SmApiHealBrep`).
  - Counts: 36 input faces, 42 made; 5 regions, 8 shells, 42 faces, 96 edges, 62 vertices. A manifold solid.
  - `SmApiBrepComputeVolume` is −156215.80176435443 or −153023.30695172012. The healed volume equals the raw one in each run.
- **How found:** a STEP corpus run (batch 54g9f3a577) gave different C++ volumes for the same encoded body in separate runs on main. Then (batch 54g9f3b2):
  - under gdb, a breakpoint on `srand` gave the path above;
  - with `time()` replaced through `LD_PRELOAD` by a fixed value, 5 values × 2 runs each, every pair of runs with the same value agreed. One value of the five gave −156215.80176435443; the other four gave −153023.30695172012.
- **Confirmed:** yes, on the C++ reference (the clean main build of the difftest binary), by the fixed-time runs above.
- **Rust port:** `GetPointsInFace` draws from the seeded `kernel.rng`, so the port gives −156215.80176435443 in every run. That is one of the two C++ values, so the two sides agree only in some C++ runs. A difftest case on this body compares by luck of the clock, and should stay out of any gate until the C++ side is seeded.
