# heal.heal_data_p2: the healer keeps a vertex tolerance that covers its gaps (2026-11-18)

- **Unit:** `heal.heal_data_p2`, `HealData::fix_tol_sizes` (`Fix_TolSizes`), its vertex branch.
- **Problem (tangency-labs/solidcraft#17739):** the healer reset a vertex to `max(def, largest edge tolerance, gain * vertex/edge gap / 2, gain * vertex/face gap - def)` and let it shrink. The check holds a vertex's gaps to half its tolerance (WR-679), and both gap terms are under twice the gap, so a vertex whose tolerance covered its gap failed it after the heal (`SmBrep` list 2, tests 0-3).
- **Change:** the reset tolerance has a floor, the gap gain times twice the largest gap, capped by the tolerance the vertex holds when its branch runs. A tolerance that met the check still does; none is raised past that value.

![Two panels of one vertex 2e-4 above the end of its edges. Left, main: the shaded disc of half the healed tolerance 2.1e-4 does not reach the edge ends, so the check fails. Right, this change: the disc of half 4.4e-4 contains them, so it passes. A dashed circle in both marks half the stated 5e-4, the cap.](heal.heal_data_p2--vertex-tolerance-floor.svg)

*Look at where the edge ends sit against the shaded disc: outside it on main, inside it with the floor.*

## Moved golden rows

Each file was replayed on `main`'s `p2.rs` and on the change, with `Kernel::default()` as the goldens use it, comparing every vertex tolerance and every check line. On every file only vertex tolerances end higher than on `main`, and only vertex gap failures disappear; none appears. For a summed block, its sum and its absolute sum move by the same amount, which equals the replayed raises; `gen0115`, `gen0234` and `fillet_group_fillet1019_rb23` were replayed value by value (nothing else moves; the `rb23` base body has a clean check, 12 faces, 28 edges, 18 vertices and the same volume enclosure on both sides). `corpus.parity` area and volume, and the mass properties of `source-file-box_w_slot-all-edges`, were rewritten by the bless within the 1e-9 rule (at most 3.3e-10 relative); topology counts are unchanged.

| Unit | Case | Value | Old | New | Why |
|---|---|---|---|---|---|
| blend.feature_executive | `fillet_group_fillet1019_rb23` | sum +2.969069e-05 | 140.4628891 | 140.4629187 | one vertex tolerance of the base body |
| blend.fillet_executive_p3 | `source-file-box_w_slot-all-edges` | sum +2.760338e-07 | 144.208144 | 144.2081442 | input `box_w_slot` raises |
| blend.fillet_executive_p3 | `stubs4-open_shell-e0-r0.5` | sum +5.268956e-05 | -1097.56151 | -1097.561457 | input `open_shell` raises |
| blend.fillet_executive_p3 | `stubs4-open_shell-e5-r0.5` | sum +5.268956e-05 | -1097.56151 | -1097.561457 | input `open_shell` raises |
| corpus.parity | `Fillet__bolt.smb` | check | 8 | 0 | 8 vertex gap lines gone |
| corpus.parity | `Fillet__fillet1017.smb` | check | 4 | 0 | 4 vertex gap lines gone |
| corpus.parity | `Fillet__fillet1018.smb` | check | 4 | 0 | 4 vertex gap lines gone |
| corpus.parity | `Fillet__fillet1019.smb` | check | 13 | 11 | 2 vertex gap lines gone |
| corpus.parity | `Fillet__open_shell.smb` | check | 10 | 5 | 5 vertex gap lines gone |
| corpus.parity | `Fillet__vase.smb` | check | 4 | 0 | 4 vertex gap lines gone |
| topology.brep_p10 | `gen0115` | sum +4.372919e-05 | 2018.195273 | 2018.195317 | vertices 2 and 13 (`open_shell`, manifold patch) |
| topology.brep_p10 | `gen0234` | sum +4.067860e-05 | 24.32606018 | 24.32610086 | 1 vertex raise (`bottle.smb`, zone tol 0.01) |
| topology.brep_p10 | `read_Fillet_bolt` | sum +4.377395e-04 | 2286.644374 | 2286.644812 | 4 vertex raises (`bolt`) |
| topology.brep_p10 | `read_Fillet_box_w_slot` | sum +2.760338e-07 | 145.0327861 | 145.0327864 | 4 vertex raises (`box_w_slot`) |
| topology.brep_p10 | `read_Fillet_fillet1017` | sum +7.232928e-04 | 2245.601319 | 2245.602042 | 2 vertex raises (`fillet1017`) |
| topology.brep_p10 | `read_Fillet_fillet1018` | sum +1.383058e-05 | 1064.589638 | 1064.589652 | 2 vertex raises (`fillet1018`) |
| topology.brep_p10 | `read_Fillet_fillet1019` | sum +4.464438e-05 | 420.3423546 | 420.3423993 | 1 vertex raise (`fillet1019`) |
| topology.brep_p10 | `read_Fillet_open_shell` | sum +5.268956e-05 | 2018.195264 | 2018.195317 | vertices 2 and 13 (`open_shell`) |
| topology.brep_p10 | `read_Fillet_vase` | sum +7.222229e-04 | 2554.715839 | 2554.716561 | 5 vertex raises (`vase`) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f0` | sum +5.268956e-05 | 173.0010026 | 173.0010553 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f0_rebuild` | sum +5.930947e-06 | 165.001806 | 165.001812 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f1` | sum +5.268956e-05 | 173.000992 | 173.0010447 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f10` | sum +4.320937e-05 | 173.0011225 | 173.0011657 | vertex 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f10_rebuild` | sum +5.930947e-06 | 164.001816 | 164.001822 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f11` | sum +5.268956e-05 | 173.000992 | 173.0010447 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f11_rebuild` | sum +5.930947e-06 | 164.0017955 | 164.0018014 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f1_rebuild` | sum +5.930947e-06 | 164.0017955 | 164.0018014 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f2` | sum +5.268956e-05 | 173.000992 | 173.0010447 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f2_rebuild` | sum +5.930947e-06 | 164.0018592 | 164.0018651 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f3` | sum +5.268956e-05 | 173.000992 | 173.0010447 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f3_rebuild` | sum +5.930947e-06 | 164.0018353 | 164.0018412 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f4` | sum +5.268956e-05 | 173.000992 | 173.0010447 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f4_rebuild` | sum +5.930947e-06 | 164.0017955 | 164.0018014 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f5` | sum +5.268956e-05 | 173.000992 | 173.0010447 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f5_rebuild` | sum +5.930947e-06 | 164.0017955 | 164.0018014 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f6` | sum +5.268956e-05 | 165.0010026 | 165.0010553 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f6_rebuild` | sum +5.930947e-06 | 165.001806 | 165.001812 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f7` | sum +5.268956e-05 | 173.000992 | 173.0010447 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f7_rebuild` | sum +5.930947e-06 | 164.0017955 | 164.0018014 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f8` | sum +5.268956e-05 | 173.000992 | 173.0010447 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f8_rebuild` | sum +5.930947e-06 | 164.0017955 | 164.0018014 | vertex 13 (rebuilt trims) |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f9` | sum +5.268956e-05 | 173.000992 | 173.0010447 | vertices 2 and 13 |
| topology.face_p3 | `own_reader_open_shell_uv_trim_f9_rebuild` | sum +5.930947e-06 | 164.0018353 | 164.0018412 | vertex 13 (rebuilt trims) |
