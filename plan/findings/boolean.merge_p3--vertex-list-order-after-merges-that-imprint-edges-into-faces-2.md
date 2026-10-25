# boolean.merge_p3: vertex list order after merges that imprint edges into faces: resolved (Issue #832, 2026-10-25)

All 8 vertex-order known mismatches (`upstream_merge_breps_operation_0..3`,
`upstream_piecewise_boxes_0`, `_1`, `_2_keep_other`, `_3_keep_other`) now pass at
`max_rel_err = 0.0`, promoting both `boolean.merge_p3` (`423/423`) and
`boolean.api_brep` (`279/287`, `0` deferred, `8` UB-175 rejected) to `green`:

1. **Operand `face_list` order in `Pool::load` (`crates/difftest/src/units/boolean_merge_p3.rs`).**
   The C++ shim builds box operands via `SmApiCreateBox` (`GetFaces()` stays in creation
   order `[z_min, z_max, x_min, x_max, y_min, y_max]`), whereas `Pool::load` exported via
   `family: 5` (`R_inf -> sh_inf` DFS walk, putting `y_min` before `x_min` and `z_min`).
   `IntersectTopology` iterates `(f1, f2)` in `face_list` order, controlling both the
   order `MakeEdgeInFaceWithVU` appends split faces to `sh_inf` and `ProtoEdge`
   orientation (`pOldF` vs `pNewF`). Building operands natively via
   `primitive::api_primitives::create_box` / `make_shell_vertex` + `Brep::use_arena_naming`
   matches C++'s `face_list` order 1:1.
2. **Namespace geometry owner adoption & naming-scheme awareness in `PiecewiseMerge` / `GetSurfaces` / `GetCurves`.**
   `MakeEdgeInFaceWithVU` inside `IntersectInsertRelate` creates split-face surfaces owned
   under the engine's notification namespace (`notification_key`). `MergeEngine::adopt_operand_owners`
   now runs after `Pool::intersect_insert_relate` and `Pool::process_wires` (as
   `DependencyMerge::manifold_boolean` already did), `Pool::set_operands` resets cached
   `primary_marks`/`other_marks` when switching operand handles, `Pool::new_face_brep`
   records `NamingScheme::Arena`, `copy_brep` in `boolean::merge::p3` records
   `NamingScheme::Notify`, and `topology::brep::p1::{surfaces,curves}` +
   `GeometryOwnerScope` resolve expected owners via `Brep::naming()` and
   `Brep::geometry_owner_key`, so `PiecewiseMerge` sees all 10 split faces of `Box_1`
   instead of dropping the 4 newly split faces.
