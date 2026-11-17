#### WR-793. `CreateSweepAlongPlanarPath` returns an open body, with success, at a corner too sharp for its mitre
- **Severity:** Wrong result
- **Area:** Primitive creation, sweeps
- **Location:** the eight-argument `SmPrimitiveCreation::CreateSweepAlongPlanarPath` in `source/SMLib/src/SmPrimitiveCreation.cpp`: the corner test `IsParallelTo(..., 2.0)` that decides whether a corner is mitred, and `dExtend`, which sets how far each piece runs past a corner and how far the cutting line reaches.
- **What happens:** Two causes give the same result.
  - `IsParallelTo` also counts anti-parallel tangents. A path that turns back within 2 degrees is therefore taken for a smooth joint. Its pieces are neither extended nor mitred, and the two tubes stay open at the corner, overlapping.
  - At a sharper corner the mitre needs more than `dExtend`. Each piece runs `dExtend = 2 sqrt(2) M` past the corner, where `M` is the largest absolute coordinate of the profile's box, and the cutting line reaches as far each way. The mitre of a profile `w` wide on either side of the path reaches `w / sin(beta / 2)` from the corner, where `beta` is the angle between the legs. Below `beta = 2 asin(w / (2 sqrt(2) M))`, the trim leaves the pieces open.

  In both cases the union of the pieces is not closed, and the routine returns it with success.
- **Trigger:** a circle of radius 1, capped, swept along `(0,0,0) -> (10,0,0) -> (10 - 10 cos a, 10 sin a, 0)` with tolerance 1e-3. The profile is moved onto the path.
- **How found:** tangency-labs/solidcraft#17314 (the hairpin). Measuring the issue's other rows found the sharper corners.
- **Confirmed:** on the port, which follows the C++ here. Faces/edges/vertices and volume before the change:

  | a (degrees) | circle at the origin (`M` = 1) | circle drawn at y = 3 (`M` = 4) |
  |---|---|---|
  | 0.5, 1, 1.2, 1.9 | 16/23/10, volume 0 | 16/23/10, volume 0 |
  | 2 to 3 | 22/33/16, Level 2 fails, volume 0 | |
  | 5 | 22/35/18, volume 0 | 20/33/16, volume 0 |
  | 7, 10 | 4/7/4, volume 0 | 2/3/2, volume 0 |
  | 12 to 41 | 2/2/2 (the caps alone), volume 0 | builds from 10.5 |
  | 42, 45, 90 | 4/5/3, volume 20 pi | 4/5/3, volume 20 pi |

  The predicted thresholds are 41.4 degrees for `M` = 1 and 10.1 degrees for `M` = 4. On both sides of each one the measurements agree (41 fails, 42 builds; 10 fails, 10.5 builds). At 10.5 degrees the volume equals that of the ideal mitre, 62.8624.

  Solidcore refuses a corner that turns back within 2 degrees ("the path turns back on itself at a corner"). It also refuses a sweep of a closed profile meant as a solid (both ends capped, or a closed path) whose result is not a closed manifold ("the sweep does not close"). Tests `a_corner_too_sharp_for_its_mitre_is_refused` and `a_closed_path_turning_back_where_it_closes_is_refused` (`crates/solidcore/tests/issue_2794_planar_path_mitred_elbow.rs`).
- **Not covered:** the reach itself is unchanged. A reach taken from the profile's width, instead of its coordinates, would let these corners build as mitres. The closing corner of a closed path keeps its fixed cutting line of 50.
