# surface.surface_p2, topology.brep_planar_section: goldens rows that move with the tangent curve tracer on by default (2026-11-19)

Units: `surface.surface_p2` (13 rows blessed) and `topology.brep_planar_section` (3 rows listed
as known failing). Cause: the tangential curve pass (WR-75) runs by default; the flag
`ssi_tangent_curve_tracer` is retired (tangency-labs/solidcraft#18930). Record:
`plan/upstream-issues/WR-75--addendum-2026-11-19-tracer-by-default-probe-meets.md`.

Golden values (`out`: the number of curve blocks and the sampled values `n`) on main 57b519eb6b
and at this PR's head; curve types counted from the intersections with the flag off (as main)
and on, before the retire, with the probe and node changes of this PR moving none of the rows
(the goldens at the head equal the rows blessed before them).

| Case | Golden on main | Golden at head | Curves on main | Curves with the tracer |
|---|---|---|---|---|
| `ssi_adv_ssi_bumppy_plane_f0_f0` | 91 blocks, n 23146 | 96 blocks, n 24490 | 136 crossing | 136 crossing, 24 touching |
| `ssi_adv_ssi_clasic_ruled_f0_f0` | 4 blocks, n 775 | 4 blocks, n 831 | 4 crossing | 4 crossing, 1 touching |
| `ssi_adv_ssi_cyl_cyl_sing2_f0_f0` | 18 blocks, n 4510 | 18 blocks, n 4566 | 4 crossing | 4 crossing, 1 touching |
| `ssi_adv_ssi_cyl_cyl_sing_f0_f0` | 14 blocks, n 3524 | 15 blocks, n 3636 | 6 crossing | 6 crossing, 2 touching |
| `ssi_adv_ssi_tor_purpen_tor_sing_f0_f0` | 27 blocks, n 6780 | 28 blocks, n 7004 | 14 crossing | 14 crossing, 4 touching |
| `tc_cubic_plane_line` | 1 blocks, n 114 | 13 blocks, n 3289 | 2 touching | 1 tangent |
| `tc_cubic_plane_line-swapped` | 1 blocks, n 114 | 28 blocks, n 6934 | 2 touching | 1 tangent |
| `tc_quartic_plane_circle` | empty `[0.0, 1.0]` | 20 blocks, n 5089 | none | 1 tangent |
| `tc_quartic_plane_circle-swapped` | empty `[0.0, 1.0]` | 20 blocks, n 5089 | none | 1 tangent |
| `tc_quartic_ridge_plane_line` | 1 blocks, n 114 | 4 blocks, n 949 | 2 touching | 1 tangent |
| `tc_quartic_ridge_plane_line-swapped` | 1 blocks, n 114 | 4 blocks, n 949 | 2 touching | 1 tangent |
| `tc_steinmetz_equal_cylinders` | 31 blocks, n 7844 | 32 blocks, n 7956 | 6 crossing | 6 crossing, 2 touching |
| `tc_steinmetz_equal_cylinders-swapped` | 32 blocks, n 8024 | 32 blocks, n 8136 | 6 crossing | 6 crossing, 2 touching |

Why the new values are right: each new touching point lies where crossing curves meet, and the
tracer solved it as a contact with parallel normals (`ssi_*`, `tc_steinmetz_*`, the nodes at
`(0, ±1, 0)`); the `tc_*` contact curves are the circle and lines of the tangential contact
corpus, which `issue_395_tangential_contact_corpus` checks against their closed forms. The
crossing curves of every row are unchanged.

`topology.brep_planar_section`: `upstream_torus_a_plane0`, `upstream_torus_a_plane6` and
`upstream_torus_c_plane0`. A plane at the torus' bounding box touches it at one point. On main
the section is empty; with the tracer it is a curve 0.175 long through that point, inside the
0.01 tolerance. Neither is the point, so the rows are listed in `known_failing.json` with
tangency-labs/solidcraft#19274, not blessed.
