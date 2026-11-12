# Topological vertex: two fixes become the default, 23 golden rows move (2026-11-12)

- Flags retired (tangency-labs/solidcraft#14246, batch B55):
  `is_topological_vertex_rejects_cusps_and_tangent_errors` (WR-92) and
  `topological_vertex_has_only_edge_uses` (WR-727), both read only in
  `is_topological_vertex` (`topology/vertex.rs`). Each WR record has an addendum of
  2026-11-12.
- `cargo run --profile goldens -p goldens -- --all --goldens` at the fix: 283 units,
  281 pass; `topology.vertex` changed=22 and `boolean.api_brep` changed=1, added, removed
  and broken 0 in both.
- Which flag moves which row: the survey of #14246 (one flag on at a time over the cases
  that reach it) puts the 22 `topology.vertex` rows on WR-92 and the `boolean.api_brep`
  row on WR-727. The mechanisms agree: the `topology.vertex` rows are wire chains, whose
  vertices have only edge uses, so WR-727 cannot change them; the `boolean.api_brep` row is
  a point contact on a sphere's seam, the case WR-727 is about.
- Blessing `boolean.api_brep` as a whole rewrites many float values within the goldens'
  tolerance and adds two cases main's golden does not hold (`ub175_after_editing_imprint_failure`
  and `gen0150`, both known failures of #14799 in `known_failing.json`). The unit was blessed with
  `--filter with_options_difference`, so the diff holds only the row that moves.

## WR-92: a cusp is not a removable vertex (22 rows of `topology.vertex`)

Each row is the `_topo` query (`is_topological_vertex` at the middle vertex of a chain of
two edges, `values.state[0]`) of a case from `fixtures/topology.vertex/constructed.json`
or `review.json`. In 21 of them the chain turns back on itself at the vertex: the path
along the two edges turns by 179.5 to 180 degrees, either as given (`turn180p0_*`) or
because one edge runs against its curve (`turn0p0_rev`: the second edge's curve points
straight on, so the path through the vertex turns back). In `turn179p5_rev_topo` the
reversed second edge makes the path bend by 0.5 degrees, exactly `SM_CONTINUITY_ANGLE`:
the oriented angle comes out just above the limit, as it does for `are_edges_tangent` on
the same vertex.

| rows | cases | path through the vertex | old | new |
|---|---|---|---|---|
| 3 | `turn180p0_{sml_cont,sml_reset,bsl_cont}_topo` | turns back 180 | 1 | 0 |
| 3 | `turn179p5_{sml_cont,sml_reset,bsl_cont}_topo` | turns back 179.5 | 1 | 0 |
| 1 | `turn179p5_rev_topo` | bends 0.5 (the limit) | 1 | 0 |
| 3 | `turn179p51_{sml_cont,sml_reset,bsl_cont}_topo` | turns back 179.51 | 1 | 0 |
| 4 | `scale{1em06,0p001,1000p0,1000000p0}_turn180p0_topo` | turns back 180 | 1 | 0 |
| 4 | `turn{0p0,0p3,0p49,0p499}_rev_topo` | second edge reversed: 179.5 to 180 | 1 | 0 |
| 2 | `rev_bsl_turn{179.51,180.0}_v1_topo` | turns back 179.51, 180 | 1 | 0 |
| 1 | `rev_sml_rev_turn0.49_v1_topo` | first edge reversed: 179.51 | 1 | 0 |
| 1 | `bspl_line_eq_180p0_topo` | turns back 180 | 1 | 0 |

Why right: the same golden holds each case's `_tan` query, `are_edges_tangent` at the same
vertex, which orients both tangents through the vertex. It is unchanged and answers 0 (not
tangent) at 0.5 degrees in all 22 cases (`turn0p0_rev_tan`, `turn180p0_sml_cont_tan0p5`,
`rev_bsl_turn180.0_v1_tan0.5`, ...). Upstream's `IsTopologicalVertex` compares the curves'
own tangents and lets a turn of 180 degrees pass (its comment: "gwc: is this a bug? it
counts cusps"), so it called the 21 cusps removable although joining the two edges
would fold the chain back on itself; `turn179p5_rev` passed through the same branch,
because its unoriented curve tangents are 179.5 degrees apart. Now both queries agree. No row moves the other way:
the smooth continuations and corners of the same fixture (`turn0p0_sml_cont_topo`,
`turn0p3_*`, `turn90p0_*`, ...) keep their values.

## WR-727: a point contact on a seam keeps its vertex (1 row of `boolean.api_brep`)

`with_options_difference` (`fixtures/boolean.api_brep/upstream_tests.json`): the box
(0, 0, 0)-(10, 10, 10) less the sphere centre (5, 5, 5), r 5, with `boolean_post_process`.
The sphere touches each face of the box at one point; (10, 5, 5) lies inside its seam
edge.

| record | old | new |
|---|---|---|
| goldens digest | `[[61,538]]`, n 23 | `[[63,549]]`, n 23 |
| faces, edges, vertices | 7, 13, 13 | 7, 14, 14 |
| volume | 476.40122411 | 476.40122411 |
| Level2 check | Bad Vertex Gap, offset 5 | clean |

Measured with a throwaway test (`boolean_difference` and `boolean_with_options`, same
result) at the commit before the fix and at the fix. Why right: on main the cleanup joins
the seam's two halves through the contact vertex, which moves the box face's vertex loop to
the sphere's pole, 5 off the face; `check_brep` refuses the body. With the fix the contact
vertex stays (one more edge and vertex), the volume is the analytic 1000 - 500/3 pi to
6e-10 relative, and the body checks clean. This case is now `seam-box-minus-sphere` in
`tests/boolean_api.rs` `point_contact_unions_check_clean`.

## Kernel rig

The rig's `fixes.json` listed WR-92 but never WR-727, so the rig now runs with WR-727 for
the first time. The survey found no rig case that needs either flag; the full gate runs the
rig at the fix.
