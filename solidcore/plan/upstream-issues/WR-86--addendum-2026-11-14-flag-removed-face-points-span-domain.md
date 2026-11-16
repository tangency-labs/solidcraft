Updates: WR-86

#### WR-86 (update, 2026-11-14). `face_points_span_domain` is removed; a rectangular face is sampled from its domain

`points_in_face` (`topology/face/p2.rs`, `SmFace::GetPointsInFace`) always samples a rectangularly trimmed face on
the grid of the 2026-10-31 addendum: column `c` of `n' = min(count, 30)` at `umin + (c + 1) L_u / (n' + 1)`, row `j`
of `J` at `vmin + (j + 1) L_v / (J + 1)` plus the dither. The opt-in no longer exists. Removed: the field, SolidCraft's
preset line, its entries in `kernel_cases/fixes.json`, the `issue_2260_gn_pipe_refined_range_end.rs` flag list and the
`issue_2014_upstream_fixes_flags.rs` example, and the old grid (fractions of the domain's lengths from zero, with the
column increment left from the search count). The limits the addendum lists stay: counts above 30 still plan
`min(count, 30)` columns, and the dither still comes from `kernel.rng`, one draw per point.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** 130 `topology.face_p2` rows move, all `GetPointsInFace` rows on rectangularly trimmed faces.
  `topology_face_p2::tests::rectangular_face_points_are_the_domain_grid` rebuilds every such case, samples it again
  and checks each point against the closed-form grid (u to the rounding of its additions, v within the dither's
  bound), strictly inside the face's UV domain, and that the recorded row is that sample. Upstream's grid starts at
  zero, so on a domain that does not start there its points lie outside the face; on one that does, its columns
  cover only the first half of the u range.
- **Tests:** `points_in_a_rectangular_face` expects the domain's centre `(0.5, 1)` for one point (was `(1/3, 1)`);
  `points_in_face_samples_from_the_domain` and `points_in_face_boundary_counts` run on the default kernel, and check
  one RNG draw per point against a kernel that drew the same count.
