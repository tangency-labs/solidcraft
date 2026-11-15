Updates: WR-542

#### WR-542 addendum (2026-11-14). The band check took a rail's whole run-on past an End at a smooth vertex, so rounds that never meet were refused as no room

- **Severity:** Wrong result (a valid fillet refused, `NoRoom` with `NoRoomReason::BandsOverlap`), with
  `fillet_refuse_overlapping_bands` (on in SolidCraft's fix list)
- **Area:** Fillets
- **Location:** the fork's `SmFilletExecutive::BandsClearOtherRails` with tangency-labs/smlib@d548305
  (WR-649) and the End at a smooth vertex's cap (`GetEndCapSurface`, smlib 41a39ec); not in the pinned
  SMLib.
- **What happens:** WR-649 samples a rail between its end vertices, where a corner has processed them;
  any other end keeps the rail's own interval end. An End at a smooth vertex makes its rails' ends as
  `RailXExtendedEdgeuse` vertices with no point, and places them on its cap
  (`FilletBrep::calc_rail_int_end_surface`) only in the corners' second pass, after the check. So the
  check sampled such a rail's whole traced run-on. Past an ordinary End the run-on leaves its face and
  those samples are dropped, but at a crease the face the rail rolls on runs on past the vertex. A round
  ending at a 4 degree crease of its wall, 2.5 from a round on the same face beyond the crease, was
  refused: its run-on crossed the other round's band.
- **Trigger:** SolidCraft's #16729: an arm against a wall that turns 4 degrees at x = 2.5, the arm's top
  edge along the straight wall (x 2.5..12) and its end edge at x = 0 rounded r 0.87.
- **How found:** SolidCraft.
- **Confirmed:** no C++ run was made here (the check is not in the pinned C++).
- **Rust port:** `band_rails` ends such a rail's band at the rail's crossing with the End's cap nearest
  the corner's vertex (`fillet_geom::p4::rail_crossing_nearest`, the search
  `calc_rail_int_end_surface` places the end with). Other ends no corner has placed keep the rail's own
  interval end, as before. `solidcore-ops/tests/blend_crossing.rs`,
  `a_round_ending_at_a_crease_is_not_refused_over_rounds_it_never_reaches`: the rounds build, with
  closed-form volumes, exact counts and surface kinds; with the crease at x = 0.5, where the bands do
  overlap, the same rounds are still refused `BandsOverlap`.
