Updates: WR-614

#### WR-614 (update, 2026-11-13). `boolean_coincident_strip_drops_crossing` is removed

The fix is the default and its flag is gone, together with WR-619's `imprint_skips_collapsed_definition`
(addendum of the same date): `IntersectInsertRelate` (`boolean/topology_intersector/p2.rs`) always runs
`drop_crossings_in_coincident_strips` (`coincident_strip.rs`) before `resolve_intersections`. The two are retired
together because the strip rule alone turns the default union at d = 1e-5 from upstream's `SM_ERR` into a body with
no solid region (WR-619's speck); with both it is the right solid. SolidCraft's preset and the kernel rig's
`fixes.json` had both flags on, so their results don't change.

- **Why the default path had to go:** without the rule, the strip's crossing is an edge whose two ends, 2e-4 apart at
  d = 1e-5, are only kept apart by luck of the proto-vertex capture distance (up to 1e-2); capture then decides contact.
  That blocks a sound capture bound (`boolean.api_brep` ub175, gen0150; the near-coincident pie-slice union).
- **Goldens (`boolean.api_brep`, `fixtures/boolean.api_brep/wr614_coincident_strip.json`):** A = [-2,2]x[16,20]x[0,4],
  B a 4 mm cube turned atan(0.1), its corner edge d below A's face. The exact overlap is a wedge of 20.2 d^2 (polygon
  clipping in exact rationals of the fixture's doubles): 5.05e-10 at d = 5e-6, 2.02e-9 at 1e-5, 4.545e-9 at 1.5e-5.
  | case (plain and `measured`) | before | after | exact |
  |---|---|---|---|
  | union d = 1e-5 | 1001 | 10/24/16, 1 solid, V 128.00007163867772 | 127.99999999798 |
  | union d = 5e-6 | 12 faces, no solid, V 0 | 10/24/16, 1 solid, V 128.00003582033884 | 127.999999999495 |
  | difference d = 1e-5, 5e-6 | 8 faces, V 0 | 6/12/8 (box A), V 63.999999999999986 | 63.99999999798, 63.999999999495 |
  | intersection d = 1e-5, 5e-6 | 2 sheets, no solid | empty | sliver below tolerance |
  The union volumes are 5.6e-7 and 2.8e-7 relative off, inside the integrator's requested 1e-6. No other case of any
  goldens unit moves.
- **Open:** at d = 1.5e-5 (past the 1e-5 tolerance, so neither rule applies) the union (no solid, V 0), the difference
  (V 0) and the intersection (2 sheets) are wrong before and after; their six rows are now in `known_failing.json`.
- **Tests:** `tests/wr_614_coincident_strip_crossing.rs` and `tests/wr_619_imprint_skips_collapsed_definition.rs`
  run on the default kernel; the halves that pinned upstream's `SM_ERR` and region-less union go, and the off/on
  comparisons are graded against exact volumes. `tests/wr_619_union_solid_leak.rs` moves to d = 1.5e-5, which still
  leaks.
