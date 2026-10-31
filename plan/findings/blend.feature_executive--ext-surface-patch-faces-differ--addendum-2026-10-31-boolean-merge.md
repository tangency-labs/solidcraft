# blend.feature_executive: the patch mismatches come from SmMerge on extended-surface faces (2026-10-31)

Updates: plan/findings/blend.feature_executive--ext-surface-patch-faces-differ.md

A step probe ran `ExtSurfaceRebuild`'s steps on one source face on both sides (the same
fixture, read by each side's own reader): the void estimate, `CreateExtendedSurface(dExt,
SM_CT_G1)`, `CreateFaceFromSurface`, `CopyFaces` of the source face, `SmMerge(ctx, faceBrep,
removeBrep).ManifoldBoolean(SM_BO_EXCLUSIVE_OR)`, `CopyFaces(.., TRUE)` into the temporary
patch and `SmMerge(ctx, patch, temp, tol).PiecewiseMerge(NULL, FALSE, TRUE)`. Every step up to
the boolean agrees to the bit: the void estimate, the extended surface (domain and samples),
the new face and the face copy (curves and surfaces).

1. **`ManifoldBoolean(SM_BO_EXCLUSIVE_OR)`** (`single_loop_circle_defillet_2`,
   `single_face_defillet_2`, `rb_33_defillet_8`: source face 8 of `Defeature/defillet.smb`, a
   `SmSurfOfRevolution`, `dExt` 2.8 from edge 14). The XOR of the extended B-spline face with
   the face copy gives 10 faces / 38 edges in C++ and 7 / 36 in Rust. The first five faces agree.
   C++ faces 5 and 6 lie on the surface of revolution (domain [0,1]^2); Rust's lie on the
   extended B-spline (domain [0,1] x [-4.48, 9.98]). The rest of the rebuild works from this:
   C++ `PiecewiseMerge` then fails (1001) and leaves one face, and `sm_TrimPatches` removes it.
2. **`PiecewiseMerge(NULL, FALSE, TRUE)`** (`single_face_fillet1019_5`: source face 8 of
   `Fillet/fillet1019.smb`, a cone, `dExt` 89.55 from feature face 5's edges). The XOR agrees
   (4 faces / 12 edges). Merging those four faces into the empty patch gives 6 faces / 15 edges
   in C++ and 7 / 16 in Rust. C++ face 5 lies on the cone; Rust's lies on the extended surface
   (`SmSurfOfExtrusion`). The same Rust `PiecewiseMerge` takes 83 s of the case's 83 s
   (`ExtSurfaceRebuild` total; the C++ case takes 2.6 s), so it is also a 30x slowdown.
   The same step diverges for `single_face_fillet1019_2` and `_6` on source face 3 (feature
   faces 2 and 6): 5 faces / 7 edges in C++ against 4 / 7, and 1 face / 3 edges against
   1 / 10.
3. **Not reproduced one face at a time:** `single_face_fillet1019_1` and
   `ext_dist_1e-09_box_with_large_fillet`. In the full rebuild the temporary patch holds
   the faces of several source faces before the one `PiecewiseMerge`, so the probe of single
   faces does not cover them. The same merge is the likely step, but this is not confirmed.

`blend::feature_executive` calls the merges exactly as `SmFeatureExecutive` does:
`DependencyMerge::carrying_attributes` + `manifold_boolean` (as `boolean::api_brep`), and
`PrimitiveModel` + `piecewise_merge` (as `boolean::api_brep::piecewise_merge`, with the
source tolerance). Both differences are inside the shared boolean merge: how faces on
coincident surfaces (the original analytic face and its B-spline or extrusion extension) are
resolved.
