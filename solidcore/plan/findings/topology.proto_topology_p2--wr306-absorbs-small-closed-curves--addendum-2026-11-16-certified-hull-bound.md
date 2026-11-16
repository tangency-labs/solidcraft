# topology.proto_topology_p2: addendum (2026-11-16): WR-306's absorb test is now a certified hull bound, and the flag is retired

Updates: plan/findings/topology.proto_topology_p2--wr306-absorbs-small-closed-curves.md

The finding and its addenda showed that WR-306's bound (`2 * max(m_dTol, capture distance)` in place of `2 * m_dTol`) absorbed real closed curves. `resolve_proto_edge` now keeps upstream's `2 * m_dTol` test and, past it, absorbs a self-loop proto edge only when both curve ends find one vertex definition and every definition's curve provably lies within that definition's tolerance of its position (`curve_piece_within`). See `plan/upstream-issues/WR-306--addendum-2026-11-16-flag-removed-degenerate-proto-edge-in-one-vertex.md`.

Measured at main `96280b89a` with that path as the only one:

| Case | Old flag on | Now |
| --- | --- | --- |
| radius-1e-3 circle union (`wr_619_keeps_closed_edges_on_a_vertex`) | 9 faces, 2 solid regions | 8, 15, 10, 1 solid region, valid (as with the flag off) |
| radii 2e-3, 3e-3 (`wr_306_keeps_small_closed_curves_inside_the_capture_ball`) | 3e-3: 9 faces, 2 solid regions | 8, 15, 10, 1, valid |
| `topology.proto_topology_p2` (14 rows the old bound moved) | moved | no row moves |
| `boolean.api_brep` `tess0033` | moved | no row moves |
| #1058 pleated-skins union | tolerances under 1e-4 | tolerances under 1e-4, level-2 clean |
| #1480 pleated-skins union | (flag off by default: 1.7255 mm tolerances, 12 level-2 failures) | same topology, level-2 clean, every edge end within half its vertex's tolerance |

`tess0033` and `tess0095` keep the results they have on main (no `boolean.api_brep` row moves).
