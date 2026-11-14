Updates: WR-490 (`WR-490--ellipse-trim-seam-lines-on-trimmed-domain.md`)

#### WR-490 (update, 2026-11-15). `ellipse_trim_seam_lines_span_cone_height` is removed; the seam lines stay on the domain

`sm_GetEllipseTrimIntervals` (`ellipse_trim_intervals`) evaluates its two seam lines on the trim domain's v range, as upstream, and the opt-in that ran them over the cone's whole height is gone. Removed: the field, its code path, SolidCraft's preset line, its entries in `fixes.json` and `issue_2260_gn_pipe_refined_range_end.rs`, and every test setting of it.

Why the upstream path, not the opt-in: since v1.0.1 the trim circles sit at the domain's v limits and the segments are classified against the domain (the 2026-10-27 and 2026-11-02 addenda), so the crossing WR-490 recorded as lost is no longer needed. Seam lines over the whole height add a crossing outside the domain; where the two seam lines are one line (a full sweep) the two equal crossings make a zero-length segment between two outside ones, which the interval rule keeps. With the opt-in made the only path, measured on main b0a5b080a:
- `surface.cone_p2`: 11 cases gain an interval outside the domain: `[0, 0]` or `[360, 360]` (8 `adv_trim_*` cases and `wr490_ellipse_on_cylinder_trimmed_v30`, whose `[0, 0]` is the ellipse point `(10, 0, 20)`, below the domain's `v >= 30`), `[490.08, 490.08]` (`gen0552`) and `[81.81, 167.03]` (`gen1022`). Sampling each ellipse at 7200 points and inverting onto the cone finds no point of these intervals inside the domain; the upstream path's intervals agree with the sampled inside runs in all 11 cases.
- `surface.cone_p3`: 7 `rejected_ub154_*` cases (a degenerate seam line) go from refused to a result with infinite geometry, and `ub154_fuzz09540_short_trim` gains infinite geometry.

With the upstream path no goldens row changes. SolidCraft's preset had the opt-in on; the 2026-11-02 addendum measured the record's boolean triggers equal under both settings (within 1.5e-11), and `wr490_equal_rods_and_tubes.rs` runs them on the default.

- **Tests:** `surface::cone::p2` `seam_crossing_below_a_trimmed_domain_keeps_the_ellipse` now also asserts the single interval `[acos(2/3), 360 - acos(2/3)]` (the opt-in's extra `[0, 0]` fails it); `wr490_equal_rods_and_tubes.rs`.
