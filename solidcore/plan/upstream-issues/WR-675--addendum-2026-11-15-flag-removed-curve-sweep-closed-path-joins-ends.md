Updates: WR-675

#### WR-675 (update, 2026-11-15). `curve_sweep_closed_path_joins_ends` is removed

A curve sweep whose end sections lie on its start sections leaves both caps off, so the stitch joins the side faces' end edges to their start edges (one closed tube). That is now the only path in `PrimitiveCreation::create_curve_sweep_with_placements`, and the flag is gone. Removed: the field, SolidCraft's preset line, its entry in `fixes.json`, fixture 2005's opt-ins and every test setting of it.

- **Also fixed:** the coincidence test compared only the end sections, so a path shorter than the B-rep's tolerance (whose end sections lie on its start sections too) was joined end to start into a body of no faces, reported as success. The join now also needs the path to leave the tolerance's ball about its start (`path_leaves_its_start`: a point of the path, at a Greville abscissa or the middle of a knot span, whose distance from the start less both points' enclosed de Boor rounding exceeds the tolerance); otherwise the sweep is capped as upstream caps it, and fails as upstream's does (1001).
- **Results:** `sweep.api_brep` `review_pipe_circle_path` (3 faces, 3 edges, 2 vertices to 1, 2, 1): the pipe once round a circle loses its two coincident caps.
- **Tests:** `tests/issue_2192_closed_curve_sweep.rs` (the closed tube and its cut from the torus by Pappus, and the path shorter than the tolerance).
