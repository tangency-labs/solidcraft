#### WR-759. `SmBrep::DeleteEdge` leaves `bSameOrientationFaces` stale after flipping the surviving face

- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmBrep.cpp:5872-5873, 6194-6197, 6524-6530`, `SmBrep::DeleteEdge` at v1.0.1 (`3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`).
- **What happens:** `bSameOrientationFaces` is computed before face-surface swapping (`:5872-5873`). When merging an outer loop with an inner loop (`:5901-5906`), the face owning the inner loop is forced to be `pSurvF` regardless of `pOptSurfaceToKeep`. If `pDelF` owns `pOptSurfaceToKeep` and the two faces have opposite surface normals (`bFlipFace == TRUE`), `SmBrep::DeleteEdge` swaps their surfaces and calls `pSurvF->FlipFaceOrientation()` (`:6194-6197`), inverting `pSurvF`'s primary faceuse orientation so it now matches `pDelF`'s. `bSameOrientationFaces` is never updated after that flip, so when `pDelF`'s remaining loops are moved onto `pSurvF` (`:6524-6530`), `if (!bSameOrientationFaces) pMoveLoop->m_pLU = pMoveLoop->m_pLU->GetOtherLoopuse();` flips each moved loop onto the wrong side of `pSurvF`.
- **Trigger:** Merging two coplanar faces with opposite surface normals across a shared closed loop that is inner on one face and outer on the other, passing `pDelF`'s surface as `pOptSurfaceToKeep`, when `pDelF` also carries an inner hole loop. Reached in Issue [#2927](https://github.com/tangency-labs/solidcore/issues/2927) (`flanged_spool.step` cut at `z = 130`, where the tool box's `+Z` cap face merges across `r = 35` with the spool's `-Z` step face `r in [31, 35]`, inverting the `r = 31` bore loop on the merged cap face, failing `SmBrep` Pointer Test 98 `Edgeuses share same region`, and inflating the integrated volume from `217496.967` to `609976.137`).
- **How found:** Issue [#2927](https://github.com/tangency-labs/solidcore/issues/2927), traced in `topology::brep::p4::delete_edge` during [PR #2969](https://github.com/tangency-labs/solidcore/pull/2969) and verified directly in `source/SMLib/src/SmBrep.cpp:5872-6531`.
- **Confirmed:** yes, by reading the pinned C++ source (`source/SMLib/src/SmBrep.cpp` at `3fd5b9548d19`):

  ```cpp
  // source/SMLib/src/SmBrep.cpp:5872-5873
        SmBoolean bSameOrientationFaces = (    pLU->   GetFaceuse()->GetOrientation()
                                            == pLURad->GetFaceuse()->GetOrientation() ) ;
  // ...
  // source/SMLib/src/SmBrep.cpp:6194-6197
                    if(bFlipFace )
                      {
                         pSurvF->FlipFaceOrientation();
                      }
  // ...
  // source/SMLib/src/SmBrep.cpp:6524-6530
                SmLoop *pMoveLoop = sLoops[ii];
                SER(pDelF->RemoveLoop(pMoveLoop));
                if (!bSameOrientationFaces)
                  {
                    pMoveLoop->m_pLU = pMoveLoop->m_pLU->GetOtherLoopuse();
                  }
                SER(pSurvF->InsertLoop(pMoveLoop));
  ```

  Reproduced on the direct port in `wr759_delete_edge_flipping_surviving_face_preserves_moved_inner_loop_orientation` (`crates/solidcore/src/topology/brep/p4.rs`, where a concentric hollow cylinder `r in [2, 10], h = 5` returns `520 * pi = 1633.628` and Pointer Test 98 without the toggle vs exact `480 * pi = 1507.964` with it) and on `cut z > 130` (`crates/step-brep/tests/issue_2927_spool_boolean.rs`). With the toggle applied when `flip_face_orientation` succeeds (`SmFace.cpp:10171-10198` returns `NER` only prior to flipping `m_pFU`), the merged `z = 130` cap face has exact area `3912.008850 mm^2` (`3.6e-8 mm^2` from closed form) and common-origin volume additivity within `1.3e-5 mm^3`; the `+0.290 mm^3` difference in `brep_volume` (`217496.967` vs `217496.677`) is not a kernel defect — `brep_volume` integrates `iint (z - z_0) n_z dA` (`OdeIsoCurveLength`), so shifting `z_0` to `bbox.mid().z = 65 mm` shifts the `+0.588 mm^3` origin integral by `-65 mm * 0.004596 mm^2 = -0.299 mm^3` due to the input STEP file's pre-existing `6.663e-3 mm` B-spline chamfer-edge tolerance on `z in [128, 129]`, shared identically by non-coincident cuts `z = 129.5, 130.5, 135.0`.
