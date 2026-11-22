#### WR-212. `DoFillet` returns success with a body that is not a closed manifold solid
- **Severity:** Wrong result
- **Area:**
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp`, the end of `DoFilleting`'s trim-and-merge path: `OrientTrimmedSurfaces` reports `bMaybeNotSolid`, which is ignored, and nothing after it checks the merged target. Line numbers are not given: they were not checked against `c9c5979`.
- **What happens:** an R4 round over the rim of a through hole merges into a target that is not a closed manifold solid, and `DoFillet` returns `SM_SUCCESS` for it. The callers get an invalid body as a result, not a refusal naming the edge.
- **Trigger:** a 40x20x10 block minus a through hole of r=3 centred at (20, 14.5), then `DoFillet` with radius 4 on the top long edge at y=20. The round covers y=16..20 and the hole reaches y=17.5, so the hole breaks into the round. This is SolidCraft's rig case `tests/kernel-cases/fillets/324-fillet-over-hole-overlap` (tangency-labs/solidcraft, branch `kernel-smlib`).
- **How found:** SolidCraft's kernel rig, through the tangency-labs/smlib fork (`smlib_blend` -> `SmFilletExecutive::DoFillet`); tangency-labs/solidcore#422.
- **Confirmed:** no (not reproduced with the reference build in this harness; seen on the fork's C++ in SolidCraft's rig).
- **Severity:** wrong result (an invalid body reported as success).
- **Rust port:** `FilletExecutive::do_fillet` refuses a target with an edge that is not `SmEdge::IsManifold`, with `SM_FILERR_BAD_MERGE` and the edge in the error info and in the error (`plan/findings.md`, `blend.fillet_executive_p3`). The cause of the broken shell is not found.
