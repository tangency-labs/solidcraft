# boolean.api_brep (`SmApiBrep.cpp` boolean entry points, PR #81, 2026-10-14)

- **Scope.** Thin wrappers over `SmMerge`; everything past input validation is deferred
  (`boolean.merge_p2` for the `SmMerge` constructor, `boolean.merge_p3` for
  `merge_breps` / `BooleanLists` / `BooleanTrees`). `SmMergeOptions` is co-ported in
  `boolean::merge::p1`. The C++ shim already runs real box/sphere booleans and emits
  `status, has, is_first, kept, faces, edges, vertices, regions, manifold_solid` (see
  `crates/solidcore-ffi/src/units/boolean_api_brep.rs`), so the deferred cases become
  comparisons as soon as the merge units land and `primitive.api_primitives` lets the
  runner build the same operands in Rust (today the Rust side passes empty `Brep`s).
- **Unrepresentable checks.** `NULL` operands, the same B-rep passed twice and `NULL` /
  duplicate list entries (`SmApiValidateBooleanOperands`,
  `SmApiMergeBrepsInputsAreValid`) cannot occur with owned Rust values; those upstream
  test cases (`TestSmBoolean`, `TestSmMergeBreps`) have no difftest counterpart.
- **Not owned.** `SmApiBooleanWithCurves`, `SmApiNonManifoldBoolean` and
  `SmApiPiecewiseMerge` (same file) are in no unit of `harness/callgraph/units.json`.
- **Performance.** Upstream `SmApiCreateSphere` takes 15-30 s at radius ~5e-5 (scale
  1e-4); the generator stops tiny scales at 1e-3. Not investigated further (unit
  `primitive.api_primitives`).
