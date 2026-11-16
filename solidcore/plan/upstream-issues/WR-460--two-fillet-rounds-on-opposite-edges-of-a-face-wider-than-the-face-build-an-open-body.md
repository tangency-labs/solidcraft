#### WR-460. Two fillet rounds on opposite edges of one face, together wider than the face, build an open body
- **Severity:** Wrong result
- **Area:** Blends
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp`, `SmFilletExecutive::DoFilleting` (after `CreateFilletBrep`): nothing compares the rails of solvers on edges that share a face.
- **What happens:** when the rounds on two edges of one face take more than the face is wide, their rails have crossed. `DoFilleting` merges the fillet faces anyway, and `DoFillet` returns success for a body with open edges (see WR-212 for that return).
- **Trigger:** a 14 x 6 x 10 post, `SmApiFilletEdges` on its four vertical edges, circular, r = 3.03, 3.0625, 3.5 and 4: success with a body that is not a closed manifold solid. r = 2.9 and 3.0 are closed solids.
- **How found:** SolidCraft's `fillet_max_each_reduces_only_where_the_radius_does_not_fit` (tangency-labs/solidcore#1073).
- **Confirmed:** yes, by the C++ build: the difftest `real_solid` case `box 14 x 6 x 10, edges [5, 6, 8, 10]` at r = 2.9, 3.03, 3.0625 and 3.5 agrees with the Rust port on all 4 (max_rel_err 0), so the open body is upstream's. The issue's 1001 "Unable to map new Fillet Edge back to Edge in Target Brep" for r in (3, 3.0625) was not reproduced through `SmApiFilletEdges` on either side (both return success at 3.03).
- **Fix:** none in tangency-labs/smlib (its history has no change to the executive after the release). Related: WR-320 (`FacesOutlastTheirBlends`) is the single-blend case of a round taking a whole face.
- **Rust port:** upstream by default. With `Kernel::fixes.fillet_refuse_crossing_rails` the session, after `CreateFilletBrep` and before the target is edited, refuses the fillet when a rail of one round lies inside the strip another round takes from the same face (rails sampled; the other edge shares no vertex with the first). The target stays unedited. Tests `issue_1073_*` (`crates/solidcore/tests/issue_1073_opposite_rounds_cross.rs`).
