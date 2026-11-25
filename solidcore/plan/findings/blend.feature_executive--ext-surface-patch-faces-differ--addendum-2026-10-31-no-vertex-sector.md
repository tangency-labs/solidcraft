Updates: blend.feature_executive--ext-surface-patch-faces-differ

## Addendum 2026-10-31: five of the eight cases were the UB-242 stop

- **Unit:** `blend.feature_executive`.
- **Cases now passing:** `single_face_defillet_2`, `single_face_fillet1019_5`, `single_face_fillet1019_6`, `rb_33_defillet_8`, `single_loop_circle_defillet_2`.
- **Cause:** the `ManifoldBoolean` XOR of an extended face with a copy of its source face (`ExtSurfaceRebuild`). For defillet face 8 (`FE_EDGE` 14), the extended B-spline surface folds over itself, so the intersection UV curves sit on the other preimage of the face's vertices. Eight edge-definition imprints then fail at `MakeEdgeInFace`'s `FindVertexSector` checks, on both sides (a `-Wl,--wrap` of `SmEdgeDefinition::Imprint` showed the same eight `SM_ERR_NULL_POINTER` statuses). The C++ `IntersectInsertRelate` ignores them and returns `SM_SUCCESS`. The port stopped, treating them as UB-242 half-edited imprints, so its XOR kept 7 faces where the C++ has 10. `find_vertex_sector` itself agrees with the C++. The topology and UV trim curves after the imprint are identical on both sides.
- **Fix:** such a failure edits nothing, so the port now goes on as upstream (`plan/upstream-issues/UB-242--addendum-2026-10-31-no-vertex-sector.md`). Regression test: `crates/solidcore/tests/defeature_imprint_without_vertex_sector.rs`.
- **Still failing:** `single_face_fillet1019_1`, `single_face_fillet1019_2`, `ext_dist_1e-09_box_with_large_fillet` (the `PiecewiseMerge` divergence of the boolean-merge addendum, not yet isolated), plus `attributes_add_by_faces_extrusion` (the patch-path attribute carry).
