# Goldens rows moved by retiring `brep_ray_classifies_current_trims` (WR-732, #18934)

Measured against `main` at c00b2c6962: each row the retirement moves, its first moved value
(main's, then this branch's), how many values of the row move, and why (below the table; the
WR-732 addendum of 2026-11-23 has the change itself).

57 rows.

| Unit | Case | First moved value (old → new) | Values | Why |
|---|---|---|---|---|
| `blend.feature_executive` | `fillet_each_fillet1019` | `values.base_geometry.blocks[1][0]`: 100.39631497429602 → 100.39631497420498 | 6 | comparator |
| `blend.feature_executive` | `single_face_fillet1019_3` | `values.base_geometry.blocks[1][0]`: 100.39631497429602 → 100.39631497420498 | 6 | comparator |
| `blend.fillet_executive_p3` | `stubs4-delete-edge-bottle-e8-r0.1` | `values.mass_properties[0]`: 33.092189853151744 → 33.0921897026184 | 2 | integration |
| `blend.fillet_executive_p3` | `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1` | `values.mass_properties[0]`: 38227.381745950406 → 38227.38173952555 | 10 | fillet |
| `blend.fillet_executive_p3` | `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5` | `values.mass_properties[0]`: 38192.70239711213 → 38192.70239068727 | 10 | fillet |
| `blend.fillet_executive_p3` | `stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1` | `values.mass_properties[0]`: 38235.992674532565 → 38235.99266808096 | 9 | read |
| `blend.fillet_executive_p3` | `stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1` | `values.mass_properties[0]`: 38227.38174592814 → 38227.38173950328 | 9 | fillet |
| `blend.fillet_executive_p3` | `stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5` | `values.mass_properties[0]`: 38192.70239761001 → 38192.70239118515 | 10 | fillet |
| `blend.fillet_executive_p3` | `stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1` | `values.mass_properties[0]`: 38235.992674532565 → 38235.99266808096 | 9 | read |
| `blend.fillet_executive_p3` | `stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5` | `values.mass_properties[0]`: 38235.992674532565 → 38235.99266808096 | 9 | read |
| `blend.fillet_executive_p3` | `sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1` | `values.mass_properties[0]`: 38235.992674532565 → 38235.99266808096 | 9 | read |
| `blend.fillet_executive_p3` | `sweep4-sphere_diff_3cylinders-all-r0.1` | `values.mass_properties[0]`: 38235.992674532565 → 38235.99266808096 | 9 | read |
| `blend.fillet_executive_p3` | `sweep5-sphere_diff_3cylinders-e29-r1.0` | `values.mass_properties[0]`: 38235.992674532565 → 38235.99266808096 | 9 | read |
| `corpus.parity` | `Fillet__sphere_diff_3cylinders.smb` | `values.area[0]`: 38235.99269705083 → 38235.9926466384 | 3 | read |
| `heal.stitch_p1` | `attributed_cov_cube1_advanced` | `values.out.blocks[0][0]`: 260155.00043588813 → 260155.0004410132 | 3 | stitch |
| `heal.stitch_p1` | `attributed_cov_cube1_advanced_plain` | `values.out.blocks[0][0]`: 260155.00043588813 → 260155.0004410132 | 3 | stitch |
| `heal.stitch_p1` | `attributed_cov_cube1_stitch` | `values.out.blocks[0][0]`: 281196.00041655113 → 281196.0004216762 | 3 | stitch |
| `heal.stitch_p1` | `attributed_cov_cube4_advanced` | `values.out.blocks[0][0]`: 260156.50052897414 → 260156.50053020142 | 3 | stitch |
| `heal.stitch_p1` | `attributed_cov_cube4_advanced_plain` | `values.out.blocks[0][0]`: 260156.50052897414 → 260156.50053020142 | 3 | stitch |
| `heal.stitch_p1` | `attributed_cov_cube4_stitch` | `values.out.blocks[0][0]`: 281185.5005094918 → 281185.5005107191 | 3 | stitch |
| `heal.stitch_p1` | `attributed_cov_cube5_advanced` | `values.out.blocks[0][0]`: 259991.6003810251 → 259991.60038550975 | 3 | stitch |
| `heal.stitch_p1` | `attributed_cov_cube5_advanced_plain` | `values.out.blocks[0][0]`: 259991.6003810251 → 259991.60038550975 | 3 | stitch |
| `heal.stitch_p1` | `attributed_cov_cube5_stitch` | `values.out.blocks[0][0]`: 281012.60036185686 → 281012.60036634153 | 3 | stitch |
| `heal.stitch_p1` | `cov_cube1_advanced` | `values.out.blocks[0][0]`: 711.0004358881436 → 711.000441013195 | 3 | stitch |
| `heal.stitch_p1` | `cov_cube1_advanced_plain` | `values.out.blocks[0][0]`: 711.0004358881436 → 711.000441013195 | 3 | stitch |
| `heal.stitch_p1` | `cov_cube1_stitch` | `values.out.blocks[0][0]`: 696.000416551165 → 696.0004216762165 | 3 | stitch |
| `heal.stitch_p1` | `cov_cube4_advanced` | `values.out.blocks[0][0]`: 692.500528974145 → 692.5005302014351 | 3 | stitch |
| `heal.stitch_p1` | `cov_cube4_advanced_plain` | `values.out.blocks[0][0]`: 692.500528974145 → 692.5005302014351 | 3 | stitch |
| `heal.stitch_p1` | `cov_cube4_stitch` | `values.out.blocks[0][0]`: 676.5005094917797 → 676.5005107190696 | 3 | stitch |
| `heal.stitch_p1` | `cov_cube5_advanced` | `values.out.blocks[0][0]`: 549.6003810251179 → 549.6003855097586 | 3 | stitch |
| `heal.stitch_p1` | `cov_cube5_advanced_plain` | `values.out.blocks[0][0]`: 549.6003810251179 → 549.6003855097586 | 3 | stitch |
| `heal.stitch_p1` | `cov_cube5_stitch` | `values.out.blocks[0][0]`: 536.6003618568992 → 536.6003663415398 | 3 | stitch |
| `heal.stitch_p2` | `cov_cube1_do` | `values.out.blocks[0][0]`: 711.0004358881436 → 711.000441013195 | 3 | stitch |
| `heal.stitch_p2` | `cov_cube1_do_sf` | `values.out.blocks[0][0]`: 711.0004358881436 → 711.000441013195 | 3 | stitch |
| `heal.stitch_p2` | `cov_cube4_do` | `values.out.blocks[0][0]`: 692.500528974145 → 692.5005302014351 | 3 | stitch |
| `heal.stitch_p2` | `cov_cube4_do_cand_v` | `values.out.blocks[0][0]`: 692.500528974145 → 692.5005302014351 | 3 | stitch |
| `heal.stitch_p2` | `cov_cube4_do_sf` | `values.out.blocks[0][0]`: 692.500528974145 → 692.5005302014351 | 3 | stitch |
| `heal.stitch_p2` | `cov_cube5_do` | `values.out.blocks[0][0]`: 549.6003810251179 → 549.6003855097586 | 3 | stitch |
| `heal.stitch_p2` | `cov_cube5_do_cand_v` | `values.out.blocks[0][0]`: 549.6003810251179 → 549.6003855097586 | 3 | stitch |
| `heal.stitch_p2` | `cov_cube5_do_sf` | `values.out.blocks[0][0]`: 549.6003810251179 → 549.6003855097586 | 3 | stitch |
| `heal.stitch_p2` | `cov_cube_gap_at_edge_tol_nest` | `values.out.blocks[0][0]`: 394.000439427135 → 394.00039863469743 | 3 | stitch |
| `topology.brep_cutting` | `gen0049` | `values.geometry.blocks[2][0]`: 85.31300971292144 → 85.31300973382875 | 3 | comparator |
| `topology.brep_cutting` | `gen0050` | `values.geometry.blocks[2][0]`: 770.8042376225555 → 770.8046844408807 | 3 | gen0050 |
| `topology.brep_p10` | `gen0026` | `values.dbls.blocks[1][0]`: 147.61687130409413 → 147.61687134896425 | 9 | comparator |
| `topology.brep_p10` | `gen0298` | `values.dbls.blocks[3][0]`: 6430.013034132512 → 7276.125376729277 | 6 | gen0298 |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f0` | `values.out.blocks[2][0]`: 196.0 → 200.0 | 6 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f1` | `values.out.blocks[2][0]`: 196.0 → 200.0 | 6 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f10` | `values.out.blocks[2][0]`: 186.0 → 190.0 | 7 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f11` | `values.out.blocks[2][0]`: 188.0 → 192.0 | 3 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f2` | `values.out.blocks[2][0]`: 186.0 → 190.0 | 7 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f3` | `values.out.blocks[2][0]`: 188.0 → 192.0 | 3 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f4` | `values.out.blocks[2][0]`: 194.0 → 198.0 | 4 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f5` | `values.out.blocks[2][0]`: 194.0 → 198.0 | 7 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f6` | `values.out.blocks[0][0]`: 140.00616589570473 → 140.00616589570294 | 6 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f7` | `values.out.blocks[2][0]`: 198.0 → 202.0 | 3 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f8` | `values.out.blocks[2][0]`: 196.0 → 200.0 | 6 | read |
| `topology.face_p3` | `own_reader_sphere_diff_3cylinders_uv_trim_f9` | `values.out.blocks[2][0]`: 196.0 → 200.0 | 6 | read |

Why:

- **read**: the read of `Fillet/sphere_diff_3cylinders.smb` holds #20179's five UV trims, made by the ray's implant
  (control points up to 2.7e-9 off in UV), and computes four of the gaps setting them clears again. On main's read
  (`tolsub::state::strip_classified_trims`) each row is main's to the bit (`tolsub::replay`, `refit::replay`); moved
  mass entries are classified EQUAL against certified enclosures (`wr493_moved_rows_are_classified`).
- **fillet**: the mass of the `e1`/`e4` fillets moves through the fillet's own classifications while it edits (up to
  1.7e-10 of the area), even on main's read; without the mass the row is main's (`KEPT_MAIN_ROW`), and the moved
  entries are classified EQUAL.
- **stitch**: vertices end at twice their gap to their edge uses' starts (`topology::face::p3`'s UV trim gaps; main kept
  the gap plus the zone tolerance), and the nest case moves four edges and three vertices; every moved tolerance is a
  row of `certify::trim_tolerance`, certified there.
- **integration**: 4.5e-9 relative; certified as an integration (`certify::trim_tolerance` OTHER) and within its
  certified enclosure (`bottle_e8_fillet_moved_by_the_lazy_raise_stays_within_its_proved_bounds`).
- **gen0050** (`Solids/nurb_fillet.smb` cut by a plane): edge 8 and its end vertices from 1.03e-3 to 1.48e-3, vertex
  32 from 5.9e-4 to 1.19e-3; measured, not certified (edge 8's trim bound is not enclosed, vertex 32's old tolerance
  covered its edge ends).
- **gen0298** (`Shell/problemShell16.smb`, `zone_tol -1`, `scale -1`): four tolerances roughly double; vertex 4 now
  covers its edge 11's end at 171.2, which its old tolerance did not.
- **comparator**: moves within the goldens' comparator; the line is written again from the run because
  `certify::trim_tolerance` or `wr450_rows` replays it bit for bit.
