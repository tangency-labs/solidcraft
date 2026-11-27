# A glued edge use kept a trim curve its own sector search had shortened (#18294)

Status: fixed in `topology/brep/p7.rs` (`glue_edges_general_searching`), found when
`FilletSession::do_filleting` began to run in `do_fillet`'s `NetScope` (tangency-labs/solidcraft#18294).

## Mechanism

`GlueEdgesGeneral` compares the surviving edge and the edge to delete once, before it places
the pairs of edge uses (`same_geometry`: the same interval and curves within a thousandth of the
edge tolerance). With the same geometry a pair keeps its UV trim curve. Placing a pair
(`find_radial_sector`) evaluates its binormal, and an edge use without a trim curve gets one
there (`create_uv_trim_curve`). When that drop comes back shorter than the edge, the drop's
recovery (`OnePercentEdgeTrimRecovery2`) trims the edge the curve was made on, the edge to
delete, to the curve. The pair then moved onto the surviving edge with a trim curve shorter
than that edge: `SmEdgeuse` test #25, "An Edgeuse UVTrimCurve->Ivl != Edge->Ivl".

![Left: the surviving edge and the edge to delete share the interval [0, t]; the trim curve made on the edge to delete while the pair is placed ends at t − δ, and the drop's recovery trims that edge to it. Right: the pair moves onto the surviving edge, still [0, t], with its trim curve [0, t − δ], which fails test #25.](topology.brep_p7--glue-keeps-a-trim-curve-its-sector-search-shortened-18294.svg)

*The trim curve made while the pair is placed is shorter than the surviving edge it moves to (δ drawn large).*

Seen on `issue_880_half_arc_rim_chamfer`'s half rim chamfer, stitched inside the fillet's net
scope: the chamfer's rail, edge 38, glued onto the target's edge 29, both [0, 5.668362735589588].
On the chamfer's net `DropCurve` failed at the rail's end, `DropAndTrimCurve` returned
[0, 5.668362368786783] (3.7e-7 short), edge 38 was trimmed to it, and the curve moved onto edge 29.

Now a pair keeps its trim curve only while the curve's interval is the surviving edge's,
compared as the edges are (`trim_curve_spans`); otherwise it is dropped as for different
geometry and made again on the surviving edge when it is asked for. That later drop's recovery
trims the surviving edge and its other edge uses' curves together.

Dropping onto S inside the scope instead (`NetScope::exact` in `create_uv_trim_curve`) also
fixed the chamfer, but `fillet_edges`' pinned blend counts read those drops on the net:
`wr698_gn_round_is_its_definition::a_round_on_a_reopened_round_lies_on_it` went from at most
65 evaluations and 16 solves to 3753 and 95, and
`wr685_gn_equal_area_per_station::equal_area_roots_and_sums_are_solved_once_each` from at most
2 area roots to 4.

## Goldens moved: `blend.fillet_executive_p3`

The `issue-880-*` cases chamfer through `create_corners` then `do_filleting`, which now runs in
the scope. Nine of the unit's 798 cases moved, compared value by value against `main`
41dcd559de (`--digest-debug` output). Areas, volumes, moments, counts and statuses are
unchanged; every mass status is 0.

Tolerances, now the body's tolerance (the stitch closed those gaps without raising it):

| Case | Values | `main` | this change |
|---|---|---|---|
| `issue-880-half-arc-rim-0-setbacks-1-1` | 2 vertex, 1 edge tolerance | 6.819214871917603e-05 | 4e-05 |
| `issue-880-half-arc-rim-0-setbacks-1-2` | 2 vertex, 1 edge tolerance | 7.465631953795772e-05 | 4.153459914054793e-05 |

Order only, the same points and curves to 1e-9 (so the topology records change too):
`issue-880-half-arc-rim-0-setbacks-1-1` lists three of its ten vertices in another order;
`issue-880-half-arc-rim-whole-offsets-1-1` lists two of its ten edges swapped.

Quantities that are zero, the centroid's y by symmetry and two products of inertia to rounding,
against moments of 1.3e5:

| Case | Value | `main` | this change |
|---|---|---|---|
| `issue-880-half-arc-rim-whole-offsets-1-1` | centroid y | -2.1700477190381972e-14 | -2.1508573641955147e-14 |
| `issue-880-half-arc-rim-whole-offsets-1-1` | product of inertia 1 | 2.1742163570263968e-10 | 2.178077679093032e-10 |
| `issue-880-half-arc-rim-whole-offsets-1-1` | product of inertia 3 | -2.4235755212284073e-10 | -2.4020124543086377e-10 |
| `issue-880-half-arc-rim-whole-offsets-1.4437-1.4437` | centroid y | -2.0823015374212282e-14 | -2.0696903537728748e-14 |
| `issue-880-half-arc-rim-whole-offsets-1.4437-1.4437` | product of inertia 1 | 2.243973878967273e-10 | 2.2693663073391936e-10 |
| `issue-880-half-arc-rim-whole-offsets-1.4437-1.4437` | product of inertia 3 | -2.267856295602066e-10 | -2.2378659846753318e-10 |
| `issue-880-half-arc-rim-whole-offsets-1-2` | centroid y | -1.7741756440808104e-14 | -2.0493018445524837e-14 |
| `issue-880-half-arc-rim-whole-offsets-1-2` | product of inertia 1 | 2.4169700399629754e-10 | 2.2350211506916e-10 |
| `issue-880-half-arc-rim-whole-offsets-1-2` | product of inertia 3 | -1.6331494646618118e-10 | -2.2353187054799476e-10 |
| `issue-880-tilted-cylinder-rim-setbacks-1-2` | centroid y | 6.107944620101754e-16 | 5.141178929862808e-16 |
| `issue-880-tilted-cylinder-rim-setbacks-1-2` | product of inertia 1 | -1.7444207742666636e-12 | -2.2162397168294e-12 |
| `issue-880-tilted-cylinder-rim-setbacks-1-2` | product of inertia 3 | 7.283258796031293e-12 | 5.667208257002261e-12 |
| `issue-880-tilted-cylinder-rim-setbacks-1-1` | centroid y | 1.42751992881317e-16 | 3.5849259442942643e-16 |
| `issue-880-tilted-cylinder-rim-setbacks-1-1` | product of inertia 1 | 2.4209007108369913e-12 | 5.142273011355577e-12 |
| `issue-880-tilted-cylinder-rim-setbacks-1-1` | product of inertia 3 | 4.941727130287725e-12 | 8.359777927886062e-12 |
| `issue-880-tilted-cylinder-rim-offsets-1-1` | centroid y | -3.522665014032644e-16 | -2.325700381568689e-16 |
| `issue-880-tilted-cylinder-rim-offsets-1-1` | product of inertia 1 | 5.546704317413222e-13 | -2.8078095988273197e-13 |
| `issue-880-tilted-cylinder-rim-offsets-1-1` | product of inertia 3 | -1.4337968400032506e-11 | -1.1779132959584872e-11 |
| `issue-880-tilted-cylinder-rim-offsets-1-2` | centroid y | 1.0746377563586875e-15 | 1.12143663842864e-15 |
| `issue-880-tilted-cylinder-rim-offsets-1-2` | product of inertia 1 | 5.2788914887592e-12 | 5.086368444356448e-12 |
| `issue-880-tilted-cylinder-rim-offsets-1-2` | product of inertia 3 | 1.061115850870194e-11 | 1.1195092842564005e-11 |

No picture: no vertex, edge sample or face sample moved by more than 1e-9.
