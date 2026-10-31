Updates: WR-665

#### WR-665 addendum (2026-10-31). A two-distance chamfer takes SetFilletParameters' second offsets, so it reaches the vertex-fan split
- **Severity:** Missing feature (wrong result for its callers), as WR-665
- **Area:** Blend (fillet executive)
- **Location:** the fork's `source/SMLib/inc/SmFilletExecutive.h` / `source/SMLib/src/SmFilletExecutive.cpp` (`SetFilletParameters`, per-edge form, argument `pOptSecondRadii`), tangency-labs/smlib@f653ac9.
- **What happens:** WR-665's `SplitBlendedVertexFans` runs inside `SetFilletParameters`. A chamfer with a second offset per edge needs `SmConstantRadiusFS`'s `pdOptSecondRadius` (upstream), which upstream's `SetFilletParameters` never passes, so such a caller built its solvers itself and skipped the split: the up-down joined squares' two axis edges chamfered 1 x 1 failed with `SM_FILERR_MAKE_TOPOLOGY` (code 3) where the fork gives 6010 (#2088).
- **How found:** SolidCraft (#2088). f653ac9 adds an optional per-edge array of second offsets to `SetFilletParameters` (default `NULL`); for `SM_FS_CONST_RADIUS` it passes entry `ii` as the solver's `pdOptSecondRadius` and refuses a list that is not one per edge (`SM_ERR_INVALID_INPUT`, "second radii size mismatch").
- **Confirmed:** no C++ run of the fork was made; not reachable in the pinned C++ (`3fd5b95`), whose `SetFilletParameters` has no such argument.
- **Rust port:** `FilletParameters::second_radii` and `SolverSpec::ConstantRadius::second_radius` (`blend/fillet_executive/p2.rs`), `FilletSession::prepare_per_edge_constant_radius_with_second` (`blend/fillet_executive/session.rs`). No `UpstreamFixes` field: `second_radii: None` is the upstream call, unchanged; the split itself stays behind `fillet_corner_per_vertex_fan`.

  Tests: `crates/solidcore/tests/issue_2088_chamfer_second_offsets.rs`: the joined squares' axis edges chamfered 1 x 1 (6010) and 1 x 2 (6020), each fails (code 3) without `fillet_corner_per_vertex_fan`; one axis edge 1 x 1 (6005) and 1 x 2 (6010); a second-radius list of the wrong size is invalid input.
