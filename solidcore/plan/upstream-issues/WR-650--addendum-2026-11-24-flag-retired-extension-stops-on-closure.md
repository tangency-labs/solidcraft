Updates: WR-650

#### WR-650 (update, 2026-11-24). The closure rules are always on

With `offset_extension_as_itself` removed (WR-633 addendum of the same date), an analytic base's extension always stops at closing its turn, half the gap past either end, and that direction then wraps like a periodic one. The fillet solver's `SurfaceIntersector::extension_stops_on_closure` is always set by `c3_make_intersector`, so a trace's extension stops where it comes back onto the traced curve's other end (2026-11-03 addendum).

- **Tests:** `wr650_half_cylinder_extension_continues_in_quarter_turns`, `wr650_partial_cylinder_extension_closes_its_turn` (in `offset_surface.rs`); the integration tests `wr650_tabbed_plate_mitred_rounds`, `solidcraft_1986_join_cylindrical_wall` and `issue_1945_rail_uv_trim`.
- **Goldens:** certified with the WR-633 rows.
