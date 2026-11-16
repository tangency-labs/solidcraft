Updates: WR-567

#### WR-567 (update, 2026-11-16). `revolution_round_offset_collapses` is removed

`Surface::offset_collapses` and `create_offset_surface` always treat a surface of revolution of one circular arc as collapsed when the offset moves the arc onto its centre or past it (`r - d (N . k) r < tol`). The fix is now unconditional and the flag is gone. SolidCraft already ran with it on. Removed: the field `UpstreamFixes::revolution_round_offset_collapses`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it (#14246, group B surface-generic).

- **Results:** two goldens rows of `offset.offset_geometry_creation` move, gen0492 and gen1247. Each has a closed-form certificate:
  - gen0492: arc radius 5, offset 100 towards the centre, so r' = -95.
  - gen1247: radius 0.56806, offset 0.82391 towards the centre, so r' < 0.
- **Tests:** `tests/wr567_revolution_round_offset_collapses.rs` keeps `wr567_round_thinner_than_the_wall_collapses` on a default kernel. `wr567_default_fails_open_at_the_other_end`, which pinned the upstream behaviour, is gone.
