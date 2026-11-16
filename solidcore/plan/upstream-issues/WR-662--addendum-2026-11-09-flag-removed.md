Updates: WR-662

#### WR-662 (update, 2026-11-09). `offset_tangent_gap_statuses and ssi_alignment_statuses` is removed

Both flags are removed: nothing read them, since the fork's soft failures are the only path (`general--status-audit-483--addendum-2026-11-07-soft-statuses-by-default.md`). The fixes live in `have_tangent_gaps` (`crates/solidcore/src/offset/offset_geometry_creation.rs`) and `ALIGN_DIR_WITH_PREVIOUS_POINT` (`crates/solidcore/src/intersect/adv_surface_intersector.rs`). Removed: both fields and every setting of them. Goldens: no case changed.
