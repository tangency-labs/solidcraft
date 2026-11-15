Updates: WR-671

#### WR-671 (update, 2026-11-15). `offset_edges_keep_source_tolerance` is removed; offset edges keep their source edges' tolerances

`MapEdgeuses` (`offset/offset_executive/p1.rs`) always raises each offset edge it matches to an original edge to
that edge's tolerance, and the offset edge's vertices to the original edge's vertices' largest
(`keep_source_tolerances`; tolerances only grow). The opt-in described in the original record no longer exists.
Removed: the field, SolidCraft's preset line, its `fixes.json` entry, the flag check, its entry in the
`issue_2260_gn_pipe_refined_range_end.rs` and `issue_2111_offset_patch_sheet_gaps.rs` flag lists, and the flag
parameter of the `issue_2111` helpers.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** 14 rows move (measured on main fb5344be5 against this change, one body dump per row):
  - `topology.brep_local_operation` gen0004, gen0025, gen0043, gen0055, gen0065, gen0079: every vertex point, edge
    curve sample and face kind is identical; 16 to 29 edge and vertex tolerances grow, none shrinks, each from the
    B-rep's 1e-5 to its source's 2e-5. fc0006 (`Solids/nurb_fillet.smb` face 13 given the plane it lies on): status
    0 on both sides, geometry identical, 13 tolerances grow (up to 7.27e-3), none shrinks; the volume moves from
    1252.2716073 to 1252.2716115, 3.4e-9 relative.
  - `blend.fillet_executive_p3` real-box-fillet1v-r3-shell-closed and real-box-fillet4v-r3-shell-closed: geometry
    identical, 12 and 32 tolerances grow (up to 8.06e-5 from 2.06e-5), none shrinks; the golden's mass properties move
    by under 1e-7 relative (the volume integrated at 1e-9 is the same on both sides).
  - `blend.fillet_executive_p3` real-box-fillet12-r3-shell-closed-issue-460 and the open-top twin: six round edges
    move from up to 1.1e-6 off their rational samples (2.0000000, 2.0307692308, ... as main's 1.9999992196,
    2.0307672844, ...) onto them; the volumes become 3116.4836769 and 2444.4836769, which differ by exactly the
    open top's 672.0000000 (main's differed by 671.9985920).
  - `offset.offset_executive_p2` api_shell_cylinder_top_face, api_shell_cylinder_top_extend_merge_in and
    api_shell_cylinder_r5_top_open_tol_1e6: the source cylinder's edges carry 1e-5 (the B-rep 1e-6), the offset's
    edges now keep it (up to 3.2e-5), and the inner floor's circle comes back as two edges, split at 225 degrees
    (one more edge and vertex). Each result checks clean at Level 2 and is a manifold solid. The r 3 shell's volume is
    53.0145447 against the closed form pi (3^2 5 - 2.5^2 4.5) = 53.0143760 (main 53.0143780): 1.7e-4 off, within
    the surface area (221.48) times the largest edge tolerance (3.2e-5), 7.1e-3.
- **Tests:** `issue_2111_offset_patch_sheet_gaps.rs` checks every offset clean on the default kernel; its separate
  flag-on test is folded into it (the area comparison against the flag-off offset is gone with that path).
