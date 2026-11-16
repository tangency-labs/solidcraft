Updates: WR-463

#### WR-463 (update, 2026-11-09). `offset_closed_curve_trim_arc` is removed

The flag is removed: nothing read it, since `TrimExtendedEdge` took the closed-curve arc (the fork's 34eaa6b) as the only path. Its fix lives in `trim_extended_edge` in `crates/solidcore/src/offset/offset_geometry_creation.rs`; the 'as upstream by default' note in the record no longer holds. Removed: the field `UpstreamFixes::offset_closed_curve_trim_arc` and every setting of it (tests, `fixes.json`, SolidCraft's `fixes()` preset). Goldens: no case changed.
