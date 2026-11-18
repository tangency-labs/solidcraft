# heal.heal_data_p2: the healer leaves a vertex a tolerance that covers its gaps (2026-11-18)

- **Unit:** `heal.heal_data_p2`, `HealData::fix_tol_sizes` (`Fix_TolSizes`), its vertex branch.
- **Problem (tangency-labs/solidcraft#17739):** the healer reset a vertex to `max(def, largest edge tolerance, gain * vertex/edge gap / 2, gain * vertex/face gap - def)`, and the vertex could shrink. The check holds a vertex's gaps to half its tolerance (WR-679), and both gap terms are under twice the gap, so a vertex whose tolerance covered its gap failed it after the heal (`SmBrep` list 2, tests 0-3).
- **Change:** when twice the vertex's largest gap is over the reset value, the tolerance becomes the gap gain times twice the gap, as the edge branch covers the edge/face gap (tangency-labs/solidcraft#18230). As the check's own tolerance update (`validate_and_update`), it is not raised to or past `GROWTH_PAST_RATIO * LARGE_SMALL_SIZE_RATIO` times the body tolerance, the limit past which Test #82 refuses the vertex: such a gap is a misplaced vertex, not one to absorb. Without that bound, four generated `topology.brep_p10` cases with scale and zone-tolerance patches grew vertices to cover gaps of millimetres.

![Two panels of one vertex 2e-4 above the end of its edges. Left, main: the shaded disc of half the healed tolerance 2.1e-4 does not reach the edge ends, so the check fails. Right, this change: the disc of half 4.4e-4 contains them, so it passes. A dashed circle in both marks half the stated 5e-4.](heal.heal_data_p2--vertex-tolerance-floor.svg)

*Look at where the edge ends sit against the shaded disc: outside it on main, inside it with the floor.*

## Moved golden rows

Every file was replayed on `main`'s `p2.rs` and on the change, with `Kernel::default()` as the goldens use it, comparing every vertex tolerance and every check line. On the eight `Fillet/` files (`bolt`, `bottle`, `box_w_slot`, `fillet1017`, `fillet1018`, `fillet1019`, `open_shell`, `vase`) 1 to 4 vertex tolerances rise and nothing else changes; only vertex gap failures disappear and none appears. For each summed block, its sum and its absolute sum move by the same amount, which equals the replayed raises to 1e-9; `gen0115`, `gen0234`, `read_Fillet_bottle` and `fillet_group_fillet1019_rb23` were replayed value by value (nothing else moves; the `rb23` base body has a clean check, 12 faces, 28 edges, 18 vertices and the same volume enclosure on both sides). `corpus.parity` area and volume, and the mass properties of `source-file-box_w_slot-all-edges`, were rewritten by the bless within the 1e-9 rule (at most 3.3e-10 relative); topology counts are unchanged.

`stubs4-open_shell-e5-r0.5` changes result: its nine level-2 gaps (tangency-labs/solidcraft#13733) were the vertex and edge gaps the healer left uncovered, so the stitched sheet fillet was refused (1005). With both covered it passes the check and is kept; `an_open_shell_sheet_fillet_is_kept` asserts that, as for the other open sheets.

| Unit | Case | Value | Old | New | Why |
|---|---|---|---|---|---|
| blend.feature_executive | `fillet_group_fillet1019_rb23` | sum +2.969069e-05 | 140.4628891 | 140.4629187 | one vertex tolerance of the base body |
| blend.fillet_executive_p3 | `stubs4-degenerate-bottle-e0-r0.5` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-degenerate-bottle-e10-r0.5` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-degenerate-bottle-e13-r0.5` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-degenerate-bottle-e2-r0.5` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e16-r0.5` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e19-r0.5` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e27-r0.5` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e27-r2.0` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e29-r2.0` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e30-r0.1` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e30-r2.0` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e32-r0.1` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e32-r2.0` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e4-r0.5` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e6-r0.5` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e7-r0.1` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e8-r0.1` | sum +3.067860e-05 | 1012.45648 | 1012.456511 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-delete-edge-bottle-e8-r0.5` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `stubs4-open_shell-e0-r0.5` | sum +3.975800e-05 | -1097.561439 | -1097.561399 | input `open_shell` raises |
| blend.fillet_executive_p3 | `sweep4-bottle-e29-r0.1` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| blend.fillet_executive_p3 | `sweep5-bottle-e29-r1.0` | sum +3.067860e-05 | 912.7406598 | 912.7406905 | input `bottle` vertex 22 |
| corpus.parity | `Fillet__bolt.smb` | check | 8 | 0 | vertex gap lines gone |
| corpus.parity | `Fillet__bottle.smb` | check | 2 | 0 | vertex gap lines gone |
| corpus.parity | `Fillet__fillet1017.smb` | check | 4 | 0 | vertex gap lines gone |
| corpus.parity | `Fillet__fillet1018.smb` | check | 4 | 0 | vertex gap lines gone |
| corpus.parity | `Fillet__fillet1019.smb` | check | 13 | 11 | vertex gap lines gone |
| corpus.parity | `Fillet__open_shell.smb` | check | 4 | 0 | vertex gap lines gone |
| corpus.parity | `Fillet__vase.smb` | check | 4 | 0 | vertex gap lines gone |
| topology.brep_p10 | `gen0115` | sum +3.510541e-05 | 2018.195343 | 2018.195379 | `open_shell` vertices 2, 13 (manifold patch) |
| topology.brep_p10 | `gen0234` | sum +3.067860e-05 | 24.32608018 | 24.32611086 | `bottle` vertex 22 (zone tol patch) |
| topology.brep_p10 | `read_Fillet_bolt` | sum +4.377395e-04 | 2286.644374 | 2286.644812 | vertex raises of the file |
| topology.brep_p10 | `read_Fillet_bottle` | sum +3.067860e-05 | 24.32608018 | 24.32611086 | vertex raises of the file |
| topology.brep_p10 | `read_Fillet_fillet1017` | sum +7.232928e-04 | 2245.601319 | 2245.602042 | vertex raises of the file |
| topology.brep_p10 | `read_Fillet_fillet1018` | sum +1.383058e-05 | 1064.589638 | 1064.589652 | vertex raises of the file |
| topology.brep_p10 | `read_Fillet_fillet1019` | sum +4.464438e-05 | 420.3423546 | 420.3423993 | vertex raises of the file |
| topology.brep_p10 | `read_Fillet_open_shell` | sum +3.975800e-05 | 2018.195339 | 2018.195379 | vertex raises of the file |
| topology.brep_p10 | `read_Fillet_vase` | sum +6.618549e-04 | 2554.715839 | 2554.716501 | vertex raises of the file |
| topology.face_p3 | `own_reader_bottle_uv_trim_f0` | sum +3.067860e-05 | 166.0024566 | 166.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f1` | sum +3.067860e-05 | 166.0024566 | 166.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f10` | sum +3.067860e-05 | 164.0024623 | 164.0024929 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f11` | sum +3.067860e-05 | 166.0024566 | 166.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f2` | sum +3.067860e-05 | 164.0024566 | 164.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f3` | sum +3.067860e-05 | 166.0024566 | 166.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f4` | sum +3.067860e-05 | 166.0024566 | 166.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f5` | sum +3.067860e-05 | 166.0024566 | 166.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f6` | sum +3.067860e-05 | 164.0024566 | 164.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f7` | sum +3.067860e-05 | 166.0024566 | 166.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f8` | sum +3.067860e-05 | 166.0024566 | 166.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_bottle_uv_trim_f9` | sum +3.067860e-05 | 166.0024566 | 166.0024872 | `bottle` vertex 22 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f0` | sum +3.975800e-05 | 173.0010937 | 173.0011334 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f0_rebuild` | sum +5.930947e-06 | 165.0018223 | 165.0018282 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f1` | sum +3.975800e-05 | 173.0010832 | 173.0011229 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f10` | sum +3.320937e-05 | 173.0011997 | 173.0012329 | `open_shell` vertex 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f10_rebuild` | sum +5.930947e-06 | 164.0018323 | 164.0018382 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f11` | sum +3.975800e-05 | 173.0010832 | 173.0011229 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f11_rebuild` | sum +5.930947e-06 | 164.0018117 | 164.0018177 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f1_rebuild` | sum +5.930947e-06 | 164.0018117 | 164.0018177 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f2` | sum +3.975800e-05 | 173.0010832 | 173.0011229 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f2_rebuild` | sum +5.930947e-06 | 164.0018754 | 164.0018813 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f3` | sum +3.975800e-05 | 173.0010832 | 173.0011229 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f3_rebuild` | sum +5.930947e-06 | 164.0018515 | 164.0018575 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f4` | sum +3.975800e-05 | 173.0010832 | 173.0011229 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f4_rebuild` | sum +5.930947e-06 | 164.0018117 | 164.0018177 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f5` | sum +3.975800e-05 | 173.0010832 | 173.0011229 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f5_rebuild` | sum +5.930947e-06 | 164.0018117 | 164.0018177 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f6` | sum +3.975800e-05 | 165.0010937 | 165.0011334 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f6_rebuild` | sum +5.930947e-06 | 165.0018223 | 165.0018282 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f7` | sum +3.975800e-05 | 173.0010832 | 173.0011229 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f7_rebuild` | sum +5.930947e-06 | 164.0018117 | 164.0018177 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f8` | sum +3.975800e-05 | 173.0010832 | 173.0011229 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f8_rebuild` | sum +5.930947e-06 | 164.0018117 | 164.0018177 | `open_shell` vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f9` | sum +3.975800e-05 | 173.0010832 | 173.0011229 | `open_shell` vertices 2, 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f9_rebuild` | sum +5.930947e-06 | 164.0018515 | 164.0018575 | `open_shell` vertex 13 (rebuilt trims) |
| blend.fillet_executive_p3 | `stubs4-open_shell-e5-r0.5` | status, faces | 1005, 6 | 0, 7 | gaps covered: the fillet now passes the check and is kept |
