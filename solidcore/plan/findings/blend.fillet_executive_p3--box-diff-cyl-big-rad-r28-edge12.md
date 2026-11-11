# blend.fillet_executive_p3: box_diff_cyl_big_rad edge 12 at radius 28 (2026-10-26)

Unit: `blend.fillet_executive_p3` (live `FilletSession`, `SmApiBrep::FilletEdges` path).

Inputs: `Fillet/box_diff_cyl_big_rad.smb`, edge 12 (the closed circular edge, one
G1-closed `NByOneClosed` 2x1 corner), radius 28.0, xsect 1, continuity 1, thumbweight 1
(the runner's defaults: no global merge; upstream's prog_test 93 runs this edge at 28.0
with `bDoGlobalMerge = TRUE`, which the runner cannot set).

Values: C++ returns `SM_SUCCESS` (status 0); Rust fails the final closed-manifold check
(`SM_FILERR_BAD_MERGE`, status 1001, non-manifold edge 20 of the target). Up to that point
the records nearly agree: `result_topology[88]` cpp 1 / rust 2, `result_topology[92]`
cpp 2 / rust 1, `result_geometry[171..173]` cpp (35.11331119332864, 44.26674880478529,
-24.34666719436646) / rust (63.616629716780075, 48.0355050829513, -18.48000040054321).
The same edge at radius 0.5 and 2.0 passes bit-identically.

Suspected cause: not located. The closed-corner topology, mate vertex and cross-section
paths are the ones the passing radii use (no `sm_ExamineNFixFilletGeoms` adjustment,
no tangent split); an ignored `topology::brep::p3` status 1005 inside `DoFilleting`
appears only for this radius, so the divergence is likely in the merge stage. The case
is not a fixture (it would not pass); add it once the merge divergence is found.

PR: the fillet-executive p1 integration branch (closed Nx1 slice).
