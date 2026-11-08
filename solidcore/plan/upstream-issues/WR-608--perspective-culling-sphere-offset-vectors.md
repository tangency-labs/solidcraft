#### WR-608. Perspective culling uses the wrong sphere and treats points as vectors
- **Severity:** Wrong result
- **Area:** Spatial structures and small geometry
- **Location:** `source/SMLib/src/SmGlobalSolver.cpp:541–565`;
  `source/SMLib/inc/SmVector3d.h:892–921` documents/implements the vector API.
- **What happens:** sphere 2 uses sphere 1's radius and center. Both sphere loops
  pass `center + offset` to a vector projection that adds the base point itself,
  which also changes the projected radius. A point inside a second node's box can
  be culled, and separated projected boxes can be retained.
- **Trigger:** eye `(0,0,0)`, plane `z=1`, point `(1.75,0,2)` and second box segment
  `[(0,0,2),(2,0,2)]`; their projections overlap but the reference rejects the branch.
- **How found:** adversarial/edge fixture and generated native difftest cases.
- **Confirmed:** yes (`geom.global_solver/perspective_unequal_containing_swap_0`,
  `gen0207`; caller `curve.curve_p9/gen0415`; details in [findings #525](../findings/general--525-flat-corners-perspective-bounds.md), PR #684).
- **Rust port:** opt in with `Kernel::fixes.perspective_sphere_offsets` to use each sphere's own radius and center and project offset vectors. The default remains pinned C++ behavior.

WR-260, WR-445 and WR-454 are occupied on main, and the intermediate WR-589 is held by open PR #1828. WR-608 is the next number after the highest actual record on main and across the complete open-PR file inventory checked on 2026-10-31, including the reserved WR-604 follow-up for #674. Existing records and their references are preserved.
