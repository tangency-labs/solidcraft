# topology: `Brep` is no longer `Clone` (update, 2026-11-04)
Updates: plan/findings/topology--brep-clone-drops-geometry-owners.md

- **Unit:** `topology.topology` (`crates/solidcore/src/topology.rs`).
- **What changed:** tangency-labs/solidcore#2715 removes `impl Clone for Brep`, and `Clone` from every type that holds a `Brep` (`BooleanOutcome`, `BooleanWithCurves`, `Boolean2dOutcome`, `PiecewiseMergeOutcome`, `BooleanListsResult`, `CurveSweep`, `PlacedCurveSweep`, `SkinPrimitive`, `SkinFromFaces`, `FilletBrep`, `Hcr`, `BrepConstructor`). A copy without geometry owners can no longer be made by accident. Every copy says which one it is (the `# Copying` section on `Brep`):
  - `brep::p3::brep_copy` for an independent body;
  - `Brep::clone_standing_for_self` for a scratch copy addressed by the source's ids;
  - `Brep::snapshot` / `Brep::restore` (`Brep::with_rollback`) for a rollback.
- **Results that change:**
  - Three fillet-session rollbacks put back a body that owns its geometry (the fix of #2715).
  - The scratch copies of queries keep their geometry's owners and share their surfaces' cache slots, as upstream's in-place queries do. The test suites and the difftest units of every runner touched show no changed result.
  - `step-brep`'s `issue_2582_pcurve_outside_the_uv_domain` used to check an owner-less clone and pinned its 6 Level-2 failures (3 surfaces without owner, 3 edges failing `ValidateGeometry` for want of one). On a copy that keeps its owners the body has none.
