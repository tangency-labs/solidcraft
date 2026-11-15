Updates: WR-521

#### WR-521 (update, 2026-11-15). `taper_tangent_corner_planes` is removed

A taper extrude with linear-extension corners of a profile of lines and circular arcs with a sharp corner continues each wall as its tangent plane to meet its neighbour along their exact seam, and refuses a corner it cannot close. That is now the only path, and the flag is gone. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** `sweep.api_brep` `review_draft_square_h0` (a square drafted over a height of 0) is refused with `InvalidInput` (1007) where it built two coincident caps and no wall (2 faces, 4 edges, 4 vertices, no volume).
- **Tests:** `tests/issue_1449_taper_corner_rules.rs`, `tests/wr_571_draft_wall_closes_at_apex.rs`.
