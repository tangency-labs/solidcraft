# blend.fillet_solver_p5: WR-302 refused a round whose rail re-enters its face past the edge's end (2026-10-30)

Updates: plan/findings/blend.fillet_solver_p5--fork-rails-fit-their-faces.md

The WR-302 record is this finding (it never merged under an `upstream-issues` id), so the addendum is here.

- **Unit:** `blend.fillet_solver_p5` (`FilletSolver::rails_fit_their_faces_with_ops`, `crates/solidcore/src/blend/fillet_solver/p5/rails_fit.rs`), only with `Kernel::fixes.fillet_refuse_rails_leaving_faces`.
- **Defect (tangency-labs/solidcore#1580):** `LeavesPastAnEnd` was tried only for the first off-face run and the run reaching the rail's end. A run between two stretches on the face, wholly past an end of the filleted edge, was judged at its leave point (the end's side edge), failed the tangent-rollover test and refused the round with `NoRoom`. The fork's `518566f` (tangency-labs/smlib) has the same structure, so the fork has the same flaw; the port had copied it faithfully.
- **Input:** the #1024 prism with the spur's tip at y = 1.5, outline (0,0) (40,0) (40,5) (52,5) (55,1.5) (58,5) (70,5) (70,20) (0,20), height 10; the front top edge (0,0,10)-(40,0,10) rounded 2. The extended top rail (y = 2) leaves the top at x = 40, comes back through the spur (x 54.57 to 55.43) and leaves again.
- **Values:** flag off (Rust and pinned C++ `SmApiFilletEdges`): builds, removes 34.336294 = (1 - pi/4) 2^2 40. Flag on, before: refused, `NoRoom` (8). Flag on, after: builds the same body, removes 34.336294.
- **Change (inside the flag only):** a middle run is skipped when `leaves_past_an_end` holds both where it leaves the face and where it comes back. A middle run that leaves the face between the edge's ends is still refused (a V notch in the top reaching to y = 1.5 at x = 20). The first and last runs are judged as before. Flag off is unchanged; no difftest runner sets the flag.
- **Test:** `crates/solidcore/tests/issue_1580_middle_off_face_run_past_edge_end.rs` (fails without the change).
