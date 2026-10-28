Updates: WR-421

#### WR-421 (update, 2026-10-28). It is also the cause of #398 (a shell of a chamfered, filleted box)

The shell in tangency-labs/solidcore#398 (SolidCraft rig shell/1525) goes wrong because of this defect. The part is a 50 x 50 x 30 box with r10 vertical rounds, a 5 mm chamfer round its base and an r4 fillet where the chamfer meets the wall, shelled 2 mm inward. The chamfer runs round the r10 arcs, so `CalcTorusFilletGeom` traces it instead of revolving it into a cone.
- **Default behaviour (pinned C++ and the port alike).** `SmApiShellBrepFull` (`SmOffsetExecutive::ShellBrep`) reports success, open at the top or closed, with a body that is not a manifold solid and has zero volume (72 faces, 307 edges, 236 vertices). This was checked with a one-off native build of the same recipe against the v1.0.1 libraries: the part (26 faces, volume 70207.82565) and both shells agree with the port.
- **With `Kernel::fixes.fillet_chamfer_cone_on_arc`.** Both shells are manifold solids of the closed-form volume: 13701.734185 open (the fork, which has the fix, gives 13701.73) and 17823.858045 closed. None of the other `UpstreamFixes` fields alone repairs it.
- **Test:** `crates/solidcore/tests/issue_398_shell_box_rounds_chamfer_fillet.rs`.
- **Remaining:** the shell's success status on that body is recorded separately, in `plan/findings/offset.offset_executive_p2--shell-succeeds-with-non-manifold-body.md`.
- Fixes tangency-labs/solidcore#398.
