# Imprint: the end trim becomes the default, the extension stays opt-in (2026-11-12)

- Flag retired (tangency-labs/solidcraft#14246, batch B21a): `imprint_trims_edge_to_proto_vertex` (WR-452,
  addendum of 2026-11-12). `imprint_extends_edge_to_proto_vertex` (WR-453) stays opt-in: retiring it too turns a
  defeature that builds a sound body on main into a refusal (below).
- `cargo run --profile goldens -p goldens -- --all --goldens` at the fix: 283 units, 282 pass; `boolean.api_brep` changed=1, added, removed and broken 0. One row moves.

## The moved row: `boolean.api_brep/issue383_seed4_box_intersection_tangent_torus`

A box [22.5, 30] x [25, 32.5] x [22.5, 50] intersected with a torus about +Z through (20, 22.5, 25), R 10, r 2.5,
post-process on. The tube touches the box's bottom along an arc inside that face.

| measure | main | this change | exact |
|---|---|---|---|
| status | 0 | 0 | |
| record | `[[40,330]]`, n 18 | `[[42,341]]`, n 18 | |
| faces, edges, vertices | 5, 9, 6 | 5, 10, 7 | |
| volume | 165.11739824872885 | 165.11739862506818 | 165.11741893458091 |
| torus face area | 112.49071884178262 | 112.49071899304 | 112.49073341939143 |

The four planar face areas move by at most 1.6e-14 relative. Both bodies pass the level-2 check and are manifold
(`validation::soundness`, run on the result of each). The record moves because post-process now leaves one more
edge and one more vertex. The new vertex is (22.5, 32.18245836528079, 22.5): the edge where the plane x = 22.5 cuts the torus
is split where it touches the box's bottom, the end of the tube's tangent arc there (y - 22.5 = sqrt(93.75) to
2.4e-10). The two ends of that edge on y = 32.5 now sit on their vertices: those vertices' tolerances drop from
1.1576e-5 to the body's 1e-5. The two pieces lie on the same plane and torus, and their parameters meet at 8.33665106
(they differ by 2.4e-10); post-process doesn't join them again. The extra vertex is a redundant split of a smooth edge,
not a change of the shape: the volume moves by 2.3e-9 relative, the planar face areas by at most 1.6e-14. Why
post-process keeps the split is not traced.

Why right: the exact values come from an independent integral, not from the kernel. The box holds the tube's whole
height, so the volume is the integral over the box's xy rectangle of the tube's thickness 2 sqrt(r^2 - (rho - R)^2),
and the torus area the integral of both sheets' area element. With rho = R + r sin(phi) both integrands are smooth;
the quadrature (mpmath, 40 digits) splits at rho = 10 and rho = sqrt(10^2 + 2.5^2), where the arc in the rectangle
changes its bounding sides. Main is 1.253e-7 below the exact volume, this change 1.230e-7 below (areas 1.296e-7 and
1.282e-7 below), both inside the volume's requested relative accuracy of 1e-6. The sibling row with post-process off
(165.11739900123112, 1.207e-7 below) doesn't move.

## Why `imprint_extends_edge_to_proto_vertex` stays

`blend.feature_executive/fillet_group_fillet1019_rb23` defeatures the fillet group of faces {3, 6, 8} of
`Fillet/fillet1019.smb`. The input fails the check with a bad gap (an edge use 1.4845e-4 from its vertex at a
tolerance of 1.4098e-4). Measured at main's code with the two flags set each way:

| flags | defeature | result |
|---|---|---|
| neither (main's default) | built | check clean, manifold, 12 faces, 28 edges, 18 vertices |
| trim only (this change) | built | the same body |
| extension only | 1001 `NotEnclosed { extended_by: 101.6 }` at edge 14 | the bad input, unchanged |
| both | 1001 `NotEnclosed { extended_by: 101.6 }` at edge 14 | the bad input, unchanged |

The volume of the built body lies in [59975.47, 59986.07] (the enclosure of `validation::soundness`) in both built
states. Face 3 alone is refused either way, but for another reason: `TangentNeighbours { within_deg: 0.5 }` without
the extension, `NotEnclosed { extended_by: 101.6 }` with it
(`tests/issue_3218_defeature_limits.rs::a_fillet_ending_on_a_tangent_fillet_is_refused_as_tangent_neighbours`).
Faces 6 and 8 alone behave the same in all four states. So the extension, not the trim, makes this defeature refuse;
why it makes edge 14 unenclosed is not traced yet. Retiring it waits for that.

The extension also moves `tests/issue_1480_pleated_loft_union_edge_ends.rs::wr469_union_flag_alone_grows_eight_vertex_tolerances`
on the default kernel: the worst end gap from 7.4750e-6 to 7.0349e-6 and the volume by 6.4e-9 relative, with the same
topology and vertex tolerances. With the trim alone that test doesn't move.
