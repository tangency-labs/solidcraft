# blend.feature_executive: extended-surface patches differ from SMLib on generated single-face features (2026-10-31)

Unit `blend.feature_executive` (PR #1851). The 21 `prog_test` `smFeatureTest` cases pass. Of
the generated cases (`fixtures/blend.feature_executive/generated.json`), 8 disagree with the
C++. In all of them the default rebuild types (`SM_RB_EXTEND_SURFACE` base,
`SM_RB_FROM_OTHER_BREP` feature) build a patch through `SmFeatureExecutive::ExtSurfaceRebuild`.

| Case | First mismatch | C++ | Rust |
|---|---|---|---|
| `single_face_defillet_2` | patch faces of loop 1 (`record[44]`, `[45]`) | 0 | 3 |
| `rb_33_defillet_8` | patch faces (`record[28]`, `[29]`) | 0 | 3 |
| `single_loop_circle_defillet_2` | patch faces of the one loop (edge 14) | 0 | 3 |
| `single_face_fillet1019_1` | patch faces (`record[30]`, `[31]`) | 5 | 3 |
| `single_face_fillet1019_5` | patch faces (`record[29]`, `[30]`) | 4 | 3 |
| `single_face_fillet1019_2` | `record[29]` patch faces; `base_geometry[175]` | 4; 2.9690686227531955e-4 | 1; 2.81953169796288e-4 |
| `single_face_fillet1019_6` | `base_geometry[319]`, `feature0_geometry[72]` | 4.418780293104246e-4 | 4.201494500779127e-4 |
| `ext_dist_1e-09_box_with_large_fillet` | `base_geometry[179]`, `feature0_geometry[91]` | 2.1213224647375797e-5 | 2.0000019997478142e-5 |

`single_loop_circle_defillet_2` is the smallest reproduction: `Defeature/defillet.smb`,
`AddFeature(face 2, {SmFeatureLoop({edge 14}, EXTEND_SURFACE, FROM_OTHER_BREP)})`,
`DoDefeature`. On the Rust side the one source face along edge 14 (face 8, outer-loop branch)
gives 7 faces after the `SM_BO_EXCLUSIVE_OR` of its extended surface with the face copy, the
temporary patch has 7 faces, the `PiecewiseMerge` into the patch 4 and `sm_TrimPatches` leaves
3. The C++ patch ends with no face (the intermediate counts are not visible from the shim).

Suspected cause: below `ExtSurfaceRebuild`'s own control flow, which matches upstream: the
surface extension (`CreateExtendedSurface` of the source surface by the void estimate), the
`ManifoldBoolean` XOR or the per-face `PiecewiseMerge` (`create_new_breps_for_faces`). The
geometry mismatches (1e-5 to 1e-4 values in surface samples after `ShrinkGeometry`) look like
the same extension or shrink step. Not yet isolated; the next step is a shim probe that runs
the extension and the XOR on face 8 of `defillet.smb` alone.

The unit stays `blocked` until these are fixed or classified.
