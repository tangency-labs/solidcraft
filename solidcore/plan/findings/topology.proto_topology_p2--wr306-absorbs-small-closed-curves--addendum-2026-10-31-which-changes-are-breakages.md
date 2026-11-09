# topology.proto_topology_p2: addendum (2026-10-31): WR-306 and WR-214 improve tess0033 and tess0095; WR-306 breaks more0033 and small circles

Updates: plan/findings/topology.proto_topology_p2--wr306-absorbs-small-closed-curves.md

That finding listed the cases WR-306 (`Kernel::fixes.degenerate_proto_edge_in_one_vertex`) and WR-214 (`Kernel::fixes.proto_edge_self_loop_on_vertex_degenerate`) change: the radius-1e-3 circle union, and `boolean.api_brep`'s more0033, tess0033 and tess0095. Checking the results themselves sorts them:

| Case | Verdict |
| --- | --- |
| tess0033, tess0095 | **improvement** under WR-306, WR-214 or both. The flag-off result (= C++) is the broken one. |
| more0033 | **breakage** under WR-306. WR-214 alone leaves it unchanged. |
| the circle union (radius below about 3.18e-3) | **breakage** under WR-306, as the finding says. WR-214 alone leaves it unchanged. |

## tess0033 and tess0095

Probe: Rust, the runner's operands (`api_brep::boolean` union, a box and a sphere about 1e-3 across), in four settings (WR-214, WR-306) = off/off, on/off, off/on, on/on, `check_brep` Level 2 on the result (probe batches 46h1a and 46h1d). Off/off equals the pinned C++. All three other settings give the same result, so the on/on column covers each:

| Case | Both off (= C++) | WR-214, WR-306, or both on |
| --- | --- | --- |
| tess0033 | 9 faces, 16 edges, 11 vertices, 2 solid regions, manifold; volume 7.4315e-10; Level 2: 2 `Edge/Edge Gap` failures (edge end gaps 1.01e-4 and 9.38e-5 against tolerances 3.07e-5 and 2.54e-5), `ValidatePointers` failed | 7 faces, 13 edges, 10 vertices, 2 solid regions, manifold; volume 7.4311e-10; Level 2: no failures |
| tess0095 | 6 faces, 14 edges, 10 vertices, **no solid region, not manifold, volume 0**; Level 2: 11 failures (edge/vertex and vertex/face gaps 3.11e-5 against 2.85e-5, and others) | 7 faces, 13 edges, 10 vertices, 2 solid regions, manifold; volume 1.209e-9; Level 2: 3 failures |

The three failures left on tess0095 are open:
- `SmFace` test 32, coincident vertices;
- `SmFace` test 39, face loops sharing a vertex;
- `SmVertex` test 1, a vertex-face gap of 2.40e-4 against 2e-5.

The difftest mismatches on these two cases are the flags differing from a defective C++ result, not regressions. Neither flag is narrowed for them.

The WR-214 trace (probe batch 46h0) shows what is removed here: a self-loop stub 6.3e-5 to 1.0e-4 long, which upstream's split turns into the short edges that leave the gaps above. In issue #480's case, one end of the stub is the vertex (within 2.3e-9). In these two it is not: the nearest end is 1.7e-5 and 3.2e-5 away. That difference is recorded in case a narrower WR-214 is ever wanted.

## more0033

`SmApiBoolean2d` exclusive-or (operation 3) of a planar rectangle 0.013108 x 0.005336 at (0.017778, -0.012440) and a disk of radius 0.0038980 at (0.024051, -0.013936). The disk crosses the rectangle's bottom side 1.496e-3 above its centre. Expected areas:
- rectangle 6.99451e-5;
- disk 4.77334e-5;
- their overlap, a circular segment, 1.24970e-5.

The exclusive-or's two non-trivial pieces are therefore the disk minus the overlap, 3.52364e-5, and the rectangle minus the overlap, 5.74481e-5.

Probe: Rust, the runner's operands (`create_plane`, `create_planar_circle`, `boolean_2d` exclusive-or), in the four flag settings, `check_brep` Level 2 on the result (probe batch 46h1h):

| (WR-214, WR-306) | Result | Face areas |
| --- | --- | --- |
| off, off (= C++) | 2 faces, 9 edges, 7 vertices; Level 2: no failures | 3.523640e-5, 5.744804e-5: the expected pieces, to 1.3e-7 relative |
| on, off | the same | the same |
| off, on | 2 faces, 7 edges, 6 vertices; Level 2: no failures | 4.773347e-5, 6.994508e-5: the whole disk and the whole rectangle |
| on, on (SolidCraft's preset) | the same as off, on | the same as off, on |

All four results are open sheets (not manifold solids), as a 2D exclusive-or should be. With WR-306 on, each face is larger by the same 1.2497e-5, the overlap: the operands come back uncut and the shared overlap is removed from neither. The result still passes the Level-2 check, so nothing flags the wrong answer.

With WR-306 on, the intersection is lost, which is a breakage. WR-214 has no effect on this case, with or without WR-306. The cause has not been traced. The crossing points are 7.2e-3 apart: both are inside one 0.01 capture ball, and the arc and chord between them (9.2e-3 and 7.2e-3 long) are both under WR-306's 0.02 bound. But they end on two different vertex definitions, where WR-306 keeps upstream's `2 * m_dTol` bound, so how the curves are lost is not yet known.

## Status

SolidCraft's preset turns both flags on. For it, tess0033 and tess0095 improve (measured with both on). more0033 (measured with both on) and closed curves shorter than 0.02 (the circle, measured with WR-306 alone) break under WR-306. Both breakages come from WR-306, which stays bit-for-bit equal to the fork's 03e7bf1 (asked on #960). WR-214 needs no change for these cases.
