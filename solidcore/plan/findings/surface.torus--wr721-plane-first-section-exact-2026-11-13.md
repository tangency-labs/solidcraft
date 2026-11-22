# Plane and torus: the exact section in either order, 35 golden rows move (2026-11-13)

- Flag retired (tangency-labs/solidcraft#14246, batch A13c): `plane_first_torus_section_exact`
  (WR-721). The WR-721 record has an addendum of 2026-11-13.
- `cargo run --profile goldens -p goldens -- --all --goldens` at the fix: 283 units, 278 pass;
  `boolean.api_brep` changed=9, `surface.surf_of_revolution_p1` 13, `surface.surf_of_revolution_p2` 5,
  `surface.torus` 5, `topology.face_p3` 3; added, removed and broken 0. Blessing also rewrote
  `boolean.api_brep` `issue1036_coincident_skins_intersection_measured` within the goldens'
  tolerance (relative 4e-12); its output is the same before and after this change, so it keeps its
  stored value.
- Each moved row was compared with main's raw output (`--digest-debug`, the code before this change).

## Why the rows move

- **Curve rows** (`surface.torus`, `surface.surf_of_revolution_p1`, `_p2`): a plane through the
  axis of a surface of revolution with an ellipse-kind generator gives the generator rotated as
  the curve it is. The NURB is the same to 1.6e-15 (control points relative to the curve's size,
  knots); the kind goes from B-spline to ellipse and the step interval from the knot range to
  degrees. 400 points of every new curve lie on one circle to 1.4e-14 of its radius.
  `surface.torus` also writes the ellipse's frame and radii (12 values per curve): the radii equal
  the fitted radius, the centre the fitted centre, and the NURB's ends sit at the interval's angles,
  all to 6.9e-15 (an inside-out ellipse runs its angle backwards). Nothing else in these rows moves.
- **Touching faces** (`topology.face_p3`): a plane touching a torus along its top circle. With the
  plane first the section is now the torus's own circle, so the rows equal the torus-first rows
  byte for byte.
- **Booleans** (`boolean.api_brep`): boxes with a face tangent to a torus along an arc (#383
  seeds 4 and 86) and a box through a horn torus. The tangent and cut circles are exact now.
  Volumes and areas move within the accuracy the unit asks for (`VOLUME_ACCURACY` 1e-6,
  `AREA_ACCURACY` 1e-4, relative). Independent values (box n torus, axis z, by nested quadrature in
  polar coordinates, the tube parameterised by angle so each integrand is smooth between its
  breakpoints):

| value | exact | old | new |
|---|---|---|---|
| horn: volume | 33.85730137 | 33.85729695 | 33.85730139 |
| horn: z face in the tube | 10.24565845 | 10.24565697 | 10.24565844 |
| seed 4: volume | 165.1174189 | 165.1173988 | 165.1173985 |
| seed 4: bottom inside the circle | 35.31564491 | 35.31564491 | 35.31560791 |
| seed 4: bottom outside the circle | 20.93435509 | 20.93435509 | 20.93423095 |
| seed 86: volume | 104.8384101 | 104.8384186 | 104.8384186 |
| seed 86: top inside the circle | 6.733125670 | 6.733125850 | 6.733125250 |

  The horn torus moves toward the exact values; seed 4's two bottom-face pieces move away (to
  5.9e-6 relative) and no longer add up to the face (1.6e-4 short), still within 1e-4.
  With post-processing, seed 4's intersection has 9 edges and 6 vertices instead of 10 and 7: the
  edge between a side face and the torus was split where the tangent arc ended (8.340 + 0.318)
  and is now one edge of 8.658, equal to its mirror image to 2e-15. Seed 86's difference has the
  same two pieces (8 and 4 faces) listed in the other order.

## The rows (old -> new)

### `surface.torus`

- `gen0177`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.08014570647, 0.9198542935] -> [28.09158095, 331.908419], [0.08014570647, 0.9198542935] -> [28.09158095, 331.908419]; 126 -> 150 values
- `gen0180`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.4247232159, 0.5752767841] -> [153.7129157, 206.2870843], [0.4247232159, 0.5752767841] -> [153.7129157, 206.2870843]; 86 -> 110 values
- `gen0278`: 1 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [195.0818307, 350.1088775]; 47 -> 59 values
- `gen0309`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.3608683202, 0.6391316798] -> [129.6377931, 230.3622069], [0.3486584407, 0.6513415593] -> [125.0227918, 234.9772082]; 86 -> 110 values
- `gen0402`: 6 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 0.2087023601] -> [0, 76.00432841], [0.2912976399, 0.7087023601] -> [103.9956716, 256.0043284], [0.7912976399, 1] -> [283.9956716, 360], [0, 0.2087023601] -> [0, 76.00432841], [0.2912976399, 0.7087023601] -> [103.9956716, 256.0043284], [0.7912976399, 1] -> [283.9956716, 360]; 202 -> 274 values

### `surface.surf_of_revolution_p1`

- `adv_sor_torus_axial_small`: 1 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.3352703444, 0.4147296556] -> [120, 150]
- `adv_sor_torus_axial_tilted`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.1647296556, 0.8352703444] -> [60, 300], [0.25, 0.75] -> [90, 270]
- `adv_sor_torus_axial_tiny_dom`: 1 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.2326383881, 0.2673616119] -> [84.26082952, 95.73917048]
- `adv_sor_half_axial_small`: 1 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.3352703444, 0.4147296556] -> [120, 150]
- `adv_sor_half_axial_tilted`: 1 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.1647296556, 0.8352703444] -> [60, 300]
- `adv_sor_swap_axial_small`: 1 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.3352703444, 0.4147296556] -> [120, 150]
- `adv_sor_swap_axial_tilted`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.1647296556, 0.8352703444] -> [60, 300], [0.25, 0.75] -> [90, 270]
- `adv_sor_swap_axial_tiny_dom`: 1 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0.2326383881, 0.2673616119] -> [84.26082952, 95.73917048]
- `adv_sor_half_axial_neg_side`: 1 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [0, 360]
- `gen0102`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [-132.1086664, -39.12138453], [0, 1] -> [-132.1086664, -39.12138453]
- `gen0159`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [75.93882088, 191.2837919], [0, 1] -> [75.93882088, 191.2837919]
- `gen0220`: 1 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [-149.0340071, 71.6112833]
- `gen0533`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [-11.64972306, 214.3851183], [0, 1] -> [-11.64972306, 214.3851183]

### `surface.surf_of_revolution_p2`

- `gen0346`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [-67.69974064, 134.0362858], [0, 1] -> [-67.69974064, 134.0362858]
- `gen0380`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [-61.48254398, -17.79370284], [0, 1] -> [-61.48254398, -17.79370284]
- `gen0451`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [39.32212431, 282.7259406], [0, 1] -> [39.32212431, 282.7259406]
- `gen0454`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [3.408138531, 205.2739525], [0, 1] -> [3.408138531, 205.2739525]
- `gen0790`: 2 section curve(s) B-spline (14033) -> ellipse (14021), same NURB; step interval [0, 1] -> [-60.89845355, 53.00112421], [0, 1] -> [-60.89845355, 53.00112421]

### `topology.face_p3`

- `touching_torus_plane_circle_10_tol1e-6`: out[4] 12.56636082 -> 1; out[6] 2 -> 2; out[10] -2 -> -2; out[11] -1.441695737e-09 -> 2.449293598e-16; out[14] 2 -> 2; now equal to `touching_torus_plane_circle_01_tol1e-6`
- `touching_torus_plane_circle_10_tol1e-4`: out[4] 12.56523996 -> 1; out[6] 2 -> 2; out[10] -1.999999978 -> -2; out[11] -1.20381848e-06 -> 2.449293598e-16; out[14] 2 -> 2; now equal to `touching_torus_plane_circle_01_tol1e-4`
- `touching_torus_plane_circle_10_tol1e-6_nopts`: out[4] 12.56636082 -> 1; out[6] 2 -> 2; out[10] -2 -> -2; out[11] -1.441695737e-09 -> 2.449293598e-16; out[14] 2 -> 2; now equal to `touching_torus_plane_circle_01_tol1e-6_nopts`

### `boolean.api_brep`

- `issue383_seed4_box_intersection_tangent_torus`: faces/edges/vertices 5/10/7 -> 5/9/6; volume 165.1173988 -> 165.1173985; area[4] 112.490719 -> 112.4907188
- `issue383_seed4_box_intersection_tangent_torus_no_post_process`: area[5] 88.16850277 -> 88.16849936
- `issue383_seed4_box_difference_tangent_torus`: volume 1381.757595 -> 1381.757592; area[2] 20.93435509 -> 20.93423095; area[4] 35.31564491 -> 35.31560791; area[6] 88.16850277 -> 88.16849936
- `issue383_seed4_box_difference_tangent_torus_no_post_process`: volume 1381.757595 -> 1381.757592; area[2] 20.93435509 -> 20.93423095; area[4] 35.31564491 -> 35.31560791; area[6] 88.16850277 -> 88.16849936
- `issue383_seed86_box_intersection_tangent_torus_no_post_process`: area[0] 7.411193765 -> 7.411193585; area[2] 33.29959329 -> 33.29959386
- `issue383_seed86_box_difference_tangent_torus`: regions listed in another order (same counts); area[2] 6.73312585 -> 6.73312525; area[3] 7.411193765 -> 7.411193585; area[4] 33.29959329 -> 33.29959386; area[6] 43.26687415 -> 43.26687229
- `issue383_seed86_box_difference_tangent_torus_no_post_process`: regions listed in another order (same counts); area[2] 6.73312585 -> 6.73312525; area[3] 7.411193765 -> 7.411193585; area[4] 33.29959329 -> 33.29959386; area[6] 43.26687415 -> 43.26687229
- `torus_guard_horn_box_intersection`: volume 33.85729695 -> 33.85730139; area[0] 2.453822037 -> 2.453822022; area[1] 2.453822037 -> 2.453822053; area[5] 10.24565697 -> 10.24565844; area[6] 10.24565697 -> 10.24565846
- `torus_guard_horn_box_difference`: volume 20.14270304 -> 20.1426986; area[0] 2.453822037 -> 2.453822022; area[1] 2.453822037 -> 2.453822053; area[6] 7.754343032 -> 7.754341829; area[7] 7.754343032 -> 7.754341941
