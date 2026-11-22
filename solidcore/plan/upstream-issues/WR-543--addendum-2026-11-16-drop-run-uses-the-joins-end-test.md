Updates: WR-543

#### WR-543 (addendum 2026-11-16): a run of drop pieces meets end to end as the join requires

- **Defect:** `longest_drop_run` (`topology/edgeuse/p3.rs`) took two consecutive drop pieces as one run when their ends were within the drop's UV tolerances (`tol_3d / (1 + speed)`), then joined them exactly with `BSplineCurve::create_by_joining`, which refuses ends further apart than `1000 sqrt(SM_EFF_ZERO) (1 + max|P|)`. Where `tol_3d` is large against the surface's speed, the first test is the looser one: the drop of an edge of zone tolerance 3.63 on Mouse.smb (UV tolerances about 1.8) came back in two pieces 0.54 apart in UV, and the join failed with `segments not end to end`, failing the whole `create_uv_trim_curve`.
- **Fix:** both tests use one criterion, `BSplineCurve::ends_meet_for_join` (the join's own end test, now written so a NaN distance does not meet). Pieces the join refuses are not a run; the longest piece is kept on its own, as for any pieces that do not meet.
- **Found by:** the retirement of `curve_contacts_at_curve_ends` (WR-455 addendum of the same date), which moves the trace's split points within that tolerance; main has the same latent mismatch.
- **Tests:** `longest_drop_run_does_not_join_pieces_the_join_refuses` (the two Mouse pieces) and `mouse_two_struts_removal_builds_with_mains_topology` (goldens crate, the whole case).
