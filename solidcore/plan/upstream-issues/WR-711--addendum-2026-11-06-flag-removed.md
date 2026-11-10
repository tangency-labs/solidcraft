Updates: WR-711

#### WR-711 (update, 2026-11-06). `fillet_exact_arc_spans` is removed: a round's exact circles always take one arc count

Since WR-708's exact circle is now every circular round's section (WR-708's addendum of
2026-11-06), its fix is on with it: `UpstreamFixes::fillet_exact_arc_spans` is removed. Every
round of exact circles goes through `ArcCounts` (`blend/fillet_standard_solver/p1.rs`: the second
pass rebuilds each circle of fewer arcs with the most) and `collapse_point_sections`. A round
of approximated arcs, asked for explicitly, keeps its own second pass, as before. This closes
tangency-labs/solidcore#2330.

- **Results that change:** a round whose exact sections change arc count along its rails. The
  progressive cubic law `[2, 2, 6, 3, 0]` was the fork 5e8c9c1's 1.966e-3 off its balls with
  every fix but this one, and is now 8.1e-5 off. Its volume 5957.7186842 is within 3.1e-8 of
  the balls' envelope's 5957.718502; the fork's 5957.716621 was 3.2e-7 under it.
- **Unchanged:** a round whose sections all have one arc count. The linear round on its cone is
  fork 5e8c9c1's 11956.222433581395 to 1e-9, and the five cliff overflows of
  `wr685_variable_round_touch_circle` are fork 5e8c9c1's to 1e-12.
- **Needs the stations that follow the law:** the exact circles are skinned between the
  trace's stations. On a kernel without `fillet_variable_span_follows_fillet` (the fork's
  4438859), the progressive round is 5.0e-4 off its balls between stations, and its volume is
  1.2e-7 over the envelope's. With it, the round is 8.1e-5 off. `fork_ea59e9c_section_orders`'
  default-kernel case now sets that flag. SolidCraft's set has it.
- **Tests:** the flag-off half of
  `wr711_exact_circles_of_one_arc_count_keep_the_progressive_round_on_its_balls` and the fork
  5e8c9c1 kernel of `wr685_variable_round_touch_circle` are removed. The 5e8c9c1 values that
  still hold are kept on every fix.
