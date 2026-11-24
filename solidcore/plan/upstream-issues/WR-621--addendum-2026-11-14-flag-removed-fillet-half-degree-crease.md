Updates: WR-621

#### WR-621 (update, 2026-11-14). `fillet_half_degree_crease` is removed; faces are tangent only within half a degree

`blend_edgeuse` (`topology/edge.rs`, `SmEdge::GetBlendEdgeuse`) always takes two faces for tangent only within half a
degree of each other (`edge::CREASE_ANGLE`, 89.5 degrees between one face's binormal and the other's normal), and
`blend_edgeuse_or_why` and the round-too-small refusal (`blend/blend_refusal.rs`) use the same angle. The opt-in no
longer exists. Removed: the field, SolidCraft's preset line, its entries in `kernel_cases/fixes.json` and the
`issue_2260_gn_pipe_refined_range_end.rs` flag list, the 88 degree branch, and the test settings in
`issue_1945_fork_1207_step4.rs`, `issue_2020_offset_mitred_below_depth.rs`, `wr650_tabbed_plate_mitred_rounds.rs`
and `issue_1999_shallow_crease_round_contacts.rs` (whose `half_degree` profile is now the default one).

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** one `blend.fillet_executive_p3` row moves, `stubs4-degenerate-nurb_cone-e8-r0.5`, and only in its
  `fillet_point_step` key (the ND-3 latch probe: 1e-3 to the unlatched 1e-5). Its status (`SM_ERR`), counts, mass
  properties, result topology and geometry are unchanged. Edge 8 of `Fillet/nurb_cone.smb` turns by 0.606 degrees:
  upstream's 2 degree limit takes it for tangent, so the solver runs and fails at a tangent point (latching 1e-3).
  It is now a crease, refused before solving as too small to round (`RoundTooSmall`): a round of r 0.5 across a
  turn of 0.0106 rad stands `r (1 / cos(turn / 2) - 1) = 7.0e-6` off the edge, under the 1e-5 tolerance; the
  smallest radius that rounds it is `1e-5 / (1 / cos(turn / 2) - 1) = 0.715`.
- **Tests:** `wr_621_half_degree_crease_rounds.rs` runs on the default kernel;
  `wr_621_without_the_opt_in_a_one_degree_crease_is_refused`, which asserted upstream's refusal, is removed (its 3
  degree rounds are in `wr_621_creases_from_half_a_degree_round` too).
