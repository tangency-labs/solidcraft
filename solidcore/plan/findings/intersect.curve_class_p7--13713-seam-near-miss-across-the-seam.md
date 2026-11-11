# intersect.curve_class_p7: a vertex is a near miss of a seam only across it (2026-11-10)

Unit: `intersect.curve_class_p7` (`CurveClassification::has_near_misses`). Case: a dome turned
about y from a quarter disc whose arc SolidCraft drew through a point rounded to eight decimals,
so its circle has radius 10 + 1.01e-8 and its base lies 1.01e-8 above y = 0, joined flat to a
cylinder of radius 10 below it, or to its own mirror about y = 0. Both seams lie in z = 0 on +x.
Tests: `a_dome_1e_8_off_its_circle_joins_*` in `crates/solidcore-ops/tests/boolean.rs`.

**Cause.** Both unions give the closed-form volume, but `check_brep` Level 2 refused them with
face test 36 ("Face Near missing Seam"). The seam classification of each periodic face ends at
the vertex the two seams share, and `has_near_misses` counted that vertex's whole 3D gap to the
seam curve against `1000 * EFF_ZERO` = 1e-9. Measured at main 017c47a40: 1.433e-8 for the
cylinder (1.01e-8 off the dome's surface, 1.01e-8 along its seam), 2.027e-8 for the mirror (along
the seam only). Across the seam the gap is 0 in both, and the edge tolerances are 2e-5. No vertex
can lie on both seams: the cylinder's seam line crosses the dome's sphere 4.5e-4 from the circle
they share, and the mirrored seams lie on one circle but each surface stops 1.01e-8 short of
y = 0.

![The seams' plane z = 0. (a) The dome's seam ends 1.01e-8 off the surface and 1.01e-8 along the seam from the shared vertex V, which lies on the cylinder's seam: a whole gap of 1.43e-8. (b) V is the dome's seam end; the mirror's seam ends 2.03e-8 from it along the seam. Both inside the vertex tolerance 2e-5.](intersect.curve_class_p7--13713-seam-near-miss-across-the-seam/seam-gap-at-shared-vertex.svg)

*The gap at the shared vertex split into the part off the surface, along the seam and across it
(out of the page, 0 in both).*

**What test 36 is for.** It reports an edge within tolerance of the seam that is not on it
(`docs/validity-and-tolerances.md` §6): a boundary running beside the seam leaves a sliver
between them in UV, as the rounded slant plane of `issue_2538_tilted_notch_on_a_slant_edge`
does. Only a gap across the seam, within the surface, does that. A vertex off the surface or past
the seam curve's end is a gap its tolerance bounds, as in the local-tolerance model of a B-rep
(D. J. Jackson, "Boundary representation modelling with local tolerances", Proc. 3rd ACM Symp.
Solid Modeling and Applications, 1995), where an entity may lie off its geometry by up to its own
tolerance.

**Where the 1e-8 comes from.** Not from an intersection: the operands carry it. SolidCraft's
sketch hands the kernel the arc already 1e-8 off (centre at the origin, radius 10.000000010132878,
its end 1.01e-8 above the x axis), so no step of the boolean makes the gap, and no step of it can
put the shared vertex on both seams.

**Fix.** A vertex end whose gap reaches the threshold is measured again across the curve within
surface 1: the component of its offset from C(t) along n × C'(t), n the surface normal at the UV
curve 1's point (the seam's `CrvOnSurf` shares its UV curve's parameter). Where that direction is
not available (no surface 1 or UV curve 1, a vanishing tangent or normal, an evaluation that
fails) the whole gap counts, as before. The threshold is unchanged. A coincident edge's gap is
counted as upstream: in both joins only vertex ends were near misses, and an edge running beside
the seam is the sliver test 36 is for.

**What does not move.** With this change on top of the construction fix for the rounded slant
plane (the perturbation band of `issue_2538_tilted_notch_on_a_slant_edge` on that branch), all 16
rows report exactly what they report without it, the four rows still carrying face test 36
included: those are not near misses this change drops. `wr579_near_coincident_tube_ends_join`, which
pins face test 36 at tube ends 1e-5 to 1e-7 apart, passes unchanged.

**Goldens moved.** Only near-miss findings at vertex ends drop; every other output of both units
is unchanged, and the heal units that act on face properties do not move.

`topology.assert_valid`: mutation 6 moves a vertex in +x, off the surface at a cylinder's seam
vertex and along the seam at a sphere's pole. Only the face test 36 report goes; a vertex 1e-6 off
with tolerance 1e-5 becomes valid, the 0.1 and 1e-4 moves stay invalid on their vertex gap reports.

| Case | Face test 36 | Other reports | Check |
|---|---|---|---|
| `cylinder_mutation6_v0.1_i0` | 1 -> 0 | unchanged | invalid -> invalid |
| `cylinder_mutation6_v0.1_i3` | 1 -> 0 | unchanged | invalid -> invalid |
| `gen0002` | 1 -> 0 | unchanged | invalid -> invalid |
| `sphere_mutation6_v0.1_i0` | 1 -> 0 | unchanged | invalid -> invalid |
| `sphere_mutation6_v0.1_i3` | 1 -> 0 | unchanged | invalid -> invalid |
| `update_cylinder_m6_i0_v0.0001_l2` | 1 -> 0 | unchanged | invalid -> invalid |
| `update_cylinder_m6_i1_v1e-06_l2` | 1 -> 0 | unchanged | invalid -> valid |

`heal.face_props`: per face, `bad_near_miss_seam` and its direction (old -> new); nothing else.

| Case | Faces that change |
|---|---|
| `gen0055` | 0: True U -> False Neither; 1: True U -> False Neither; 2: True U -> False Neither; 3: True U -> False Neither; 4: True U -> False Neither; 5: True U -> False Neither; 6: True U -> False Neither; 7: True U -> False Neither; 8: True U -> False Neither |
| `gen0329` | 0: True Both -> True V; 1: True Both -> True V; 2: True Both -> True V |
| `gen0374` | 0: True Both -> True V; 1: True Both -> True V; 2: True Both -> True V; 3: True Both -> True V; 4: True Both -> True V |
| `gen0384` | 4: True U -> False Neither; 5: True U -> False Neither; 6: True U -> False Neither |
| `gen0435` | 0: True Both -> False Neither; 1: True Both -> False Neither; 2: True Both -> False Neither; 3: True Both -> False Neither; 4: True Both -> False Neither; 5: True Both -> False Neither; 6: True Both -> False Neither; 7: True Both -> False Neither; 8: True Both -> False Neither |
| `gen0476` | 0: True Both -> True U; 1: True Both -> True U |
| `gen0510` | 0: True U -> False Neither |
| `gen0617` | 4: True Both -> True V; 6: True Both -> True V |
| `gen0638` | 0: True Both -> False Neither; 2: True Both -> False Neither; 4: True Both -> False Neither; 5: True Both -> False Neither |
| `gen0769` | 0: True V -> False Neither; 1: True V -> False Neither; 3: True V -> False Neither; 4: True V -> False Neither; 5: True V -> False Neither; 6: True V -> False Neither; 7: True V -> False Neither |
| `gen0807` | 0: True Both -> True V; 1: True Both -> True V; 2: True Both -> True V; 3: True Both -> True V; 4: True Both -> True V; 6: True Both -> True V; 7: True Both -> True V; 8: True Both -> True V |
| `gen1014` | 0: True Both -> False Neither; 2: True Both -> False Neither |
| `gen1047` | 0: True Both -> True U; 2: True Both -> True U; 3: True Both -> True U |
| `gen1097` | 0: True Both -> True V; 1: True Both -> True V; 2: True Both -> True V |
| `gen1109` | 0: True Both -> False Neither; 2: True Both -> False Neither; 3: True Both -> False Neither; 4: True Both -> False Neither; 5: True Both -> False Neither; 6: True Both -> False Neither; 7: True Both -> False Neither; 8: True Both -> False Neither; 9: True Both -> False Neither; 10: True Both -> False Neither |
| `gen1143` | 0: True Both -> True V; 1: True Both -> True V; 2: True Both -> True V; 3: True Both -> True V; 4: True Both -> True V; 6: True Both -> True V |
| `gen1183` | 0: True Both -> True V; 1: True Both -> True V |
| `gen1354` | 0: True Both -> False Neither; 2: True Both -> False Neither; 3: True Both -> False Neither |
| `gen1365` | 0: True V -> False Neither; 4: True V -> False Neither; 5: True V -> False Neither |
| `gen1377` | 0: True Both -> True U; 2: True Both -> True U; 3: True Both -> True U; 4: True Both -> True U |
| `gen1428` | 0: True Both -> False Neither; 2: True Both -> False Neither; 3: True Both -> False Neither; 4: True Both -> False Neither |
| `gen1464` | 0: True Both -> True U; 1: True Both -> True U; 2: True Both -> True U; 3: True Both -> True U |
| `gen1482` | 0: True Both -> True V; 1: True Both -> True V; 2: True Both -> True V; 3: True Both -> True V |
| `s3gen0033` | 0: True U -> False Neither; 1: True U -> False Neither; 3: True U -> False Neither |
| `s3gen0063` | 0: True U -> False Neither; 2: True U -> False Neither |
| `s3gen0089` | 0: True U -> False Neither |
| `s3gen0099` | 0: True V -> False Neither; 1: True V -> False Neither |
| `s3gen0294` | 0: True V -> False Neither; 1: True V -> False Neither |
| `stage3_sph_one_tol1` | 0: True U -> False Neither |
| `stage3_sph_swap_one_tol1` | 0: True V -> False Neither |
| `stage3_sph_two_tol1` | 0: True U -> False Neither |
