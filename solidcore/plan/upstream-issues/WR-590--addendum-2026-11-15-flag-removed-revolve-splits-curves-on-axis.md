Updates: WR-590

#### WR-590 (update, 2026-11-15). `revolve_splits_curves_on_axis` is removed

A partial-turn rotational sweep first splits each profile curve where it meets the axis inside itself (`split_curves_on_axis`). That is now the only path, and the flag is gone. Removed: the field, the goldens runner's arm, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** no golden changes.
- **Tests:** `tests/wr_590_revolve_splits_curve_on_axis.rs` (every start of the horn disc turns into three faces of the Pappus volume).
