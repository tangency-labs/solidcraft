# General: WR-543 addendum: the fork's boundary-row sheet diverges (2026-10-31)

Updates: plan/findings/general--flag-reachability-wr543-drop-pieces-not-reached.md

The fork's capi test (e36feb1, `flat_roundtrip` `BoundaryRowTrimPace`) was exported from the fork's `smlib_brep_import` as a native file (`tests/data/fork_wr543_boundary_row_sheet.smd`). It is read without UV trims, as SolidCraft hands a face over, and `GetOrCreateUVTrimCurve` makes them. By default the boundary row's trim runs 0.85 mm off its edge (tolerance 5e-5). With the flag it lies within the edge's tolerance (0.083 against 0.159) and the edge stays whole. On the fork, reverting the hunk fails the same test. Ported as `crates/solidcore/tests/fork_e36feb1_boundary_row_trim_pace.rs` (fails with the flag forced off).
