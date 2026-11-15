Updates: WR-490 (`WR-490--ellipse-trim-seam-lines-on-trimmed-domain.md`)

#### WR-490 (update, 2026-11-15). `ellipse_trim_seam_lines_span_cone_height` is removed; the seam lines stay on the domain

`sm_GetEllipseTrimIntervals` (`ellipse_trim_intervals`) evaluates its two seam lines on the trim domain's v range, as upstream, and the opt-in that ran them over the cone's whole height is gone. Removed: the field, its code path, SolidCraft's preset line, its entries in `fixes.json` and `issue_2260_gn_pipe_refined_range_end.rs`, and every test setting of it.

Why the upstream path, not the opt-in: since v1.0.1 the trim circles sit at the domain's v limits and the segments are classified against the domain (the 2026-10-27 and 2026-11-02 addenda), so the crossing WR-490 recorded as lost is no longer needed. Seam lines over the whole height add crossings outside the domain, and two equal crossings make a zero-length segment between two outside ones, which the interval rule keeps. The crossings are equal in two ways: on a full sweep the two seam lines coincide; for a point ellipse at the apex, both extended seam lines pass through it. With the opt-in made the only path, measured on main b0a5b080a:
- `surface.cone_p2`: 11 cases gain an interval outside the domain. In 9 of them it is the single ellipse point `t ≡ 0` on the `u = 0` seam, at a height outside the domain's v range (closed form from the case data; the point's distance from the axis equals the cone's radius at that height):
  - full sweep: `adv_trim_can_s1i0` `z = 4.7020` (r 1.4702 = 1 + 0.1 z) against v `[5, 10]`; `adv_trim_can_s1i1` `z = 5.6622` (r 1.5662) against `[0, 5]`; `adv_trim_cyl_s1i0` `z = 0.3159` (r 1) against `[1, 2]`; `adv_trim_cyl_s1i1` `z = 1.999999998` (r 1) against `[0, 1]`; `adv_trim_apexb_s1i1` `z = 3.6319` (r 1.8159 = 0.5 z) against `[0, 2]`; `wr490_ellipse_on_cylinder_trimmed_v30` `(10, 0, 20)` (r 10) against `v >= 30`;
  - point ellipse at the apex (radii 0): `adv_trim_apexb_s1i0` at `v = 0` and `adv_trim_apext_s1i0` at `v = 4` against `[0.4, 2.8]`; `adv_trim_arcapex_s1i0` at `v = 0` against `[0.1, 0.7]`.
  The other two, `[490.08, 490.08]` (`gen0552`) and `[81.81, 167.03]` (`gen1022`), are measured only: sampling each ellipse at 7200 points and inverting onto the cone finds no point of them inside the domain. The upstream path's intervals agree with the sampled inside runs in all 11 cases.
- `surface.cone_p3`: 7 `rejected_ub154_*` cases (a degenerate seam line) go from refused to a result with infinite geometry, and `ub154_fuzz09540_short_trim` gains infinite geometry.

With the upstream path no goldens row changes. SolidCraft's preset had the opt-in on; the 2026-11-02 addendum measured the record's boolean triggers equal under both settings (within 1.5e-11), and `wr490_equal_rods_and_tubes.rs` runs them on the default.

- **Tests:** `surface::cone::p2` `seam_crossing_below_a_trimmed_domain_keeps_the_ellipse` now also asserts the single interval `[acos(2/3), 360 - acos(2/3)]` (the opt-in's extra `[0, 0]` fails it); `wr490_equal_rods_and_tubes.rs`.
