# General: WR-410 `boolean_vertex_by_best_crossing` moves vertices by round-off only (2026-10-31)

- **Flag:** `Kernel::fixes.boolean_vertex_by_best_crossing` (WR-410, #1059).
- **Audit:** class B. The `issue_1059_*` tests reach the guard; none fails with it forced off. `plan/upstream-issues/WR-410--addendum-2026-10-26-box-geometry-shows-no-material-effect.md` already records 32 grazing-bar placements with no material effect.
- **Added evidence (audit run, every flag on):** forcing this flag off in the SolidCraft cases changes 20 cases in 12 files (8472/* crossing cylinders, 1649/union_at_origin, 3665, a52, catics, ...), all by round-off. For example 8472/join_a_b's volume moves from 2186.0789237796425 to 2186.0789237796303 (6e-15 relative); the largest move is 1649's at 1.4e-10 relative. All verdicts are unchanged, so there is no measurable to pin above round-off.
- **Waits on:** the fork's case (rig crashes/4092, a rounded pad on a lofted cap, an edge start 1.2e-4 from its vertex), or the fork's `grazing_edge_crossing` test, once public. No fork fix is missing in front of the branch.
