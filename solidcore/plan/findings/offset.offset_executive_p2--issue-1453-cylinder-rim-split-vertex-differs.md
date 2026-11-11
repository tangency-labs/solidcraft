# offset.offset_executive_p2: the shelled r5 cylinder's extra rim vertex lands 1.2e-5 apart in Rust and C++ (2026-10-29)

**Unit:** `offset.offset_executive_p2` (`SmApiShellBrepFull`), default kernel (no `UpstreamFixes`).

**Input:** the tangency-labs/solidcore#1453 case as a fixture entry of
`fixtures/offset.offset_executive_p2/curved_primitives.json`: `"operation": "api_shell"`,
source `{"kind": "cylinder", "origin": [0, 0, 0], "radius": 5, "height": 10}`, open face
`{"origin": "source", "index": 1}` (the top), `"tolerance": 1e-05`, config
`{"offset_distance": -1.0, "extend_convex": true, "merge_results": true, "create_offset_solid": true}`.

**Result:** the topology is identical: both sides give the 7 edges and 5 vertices of
WR-495, so the C++ splits the inner rim at an extra vertex as well. The geometry
values differ in 4 of 21 (`values.result.geometry`):

| field | C++ | Rust |
|---|---|---|
| `geometry[1]` | -2.828421588629506 | -2.828433304880372 |
| `geometry[2]` | 0 | -2.8284192630706975 |
| `geometry[3]` | -1.5025837514564226e-17 | 0 |
| `geometry[5]` | 2.8284294867261224 | 6.661338147750939e-16 |

The differing values are of order 2.83 = 4 cos 45 degrees: the split points of the rim
circle (z = 10, r 4) are not the same on the two sides (one value differs by 1.2e-5,
the others by up to 2.83, so at least one point lies elsewhere on the circle). The
Rust vertex is at (-2.8284333, -2.8284193, 10). Which geometry entries belong to which
vertex was not traced.

**Suspected cause:** the marched intersection of the two G1R B-spline extensions in
`MateEdges` (WR-495) closes the rim circle at a point that depends on the march; a small
difference in the march (or in how the closing gap is split into a vertex) moves the
vertex along the circle. Not traced.

**At tolerance 1e-6** (the existing `api_shell_cylinder_top_face` case, r3 h5 at -0.5, and
the variant added by this PR, `api_shell_cylinder_r5_top_open_tol_1e6`) the rim is not split
and Rust and C++ agree.

**Status:** not added as a difftest case: a known mismatch would take the green unit to
`partial`. With `Kernel::fixes.offset_analytic_mate_edges` the rim is one exact circle and
the vertex does not exist (`crates/solidcore/tests/issue_1453_shelled_cylinder_exact_edges.rs`).
Found in tangency-labs/solidcore PR #1455.
