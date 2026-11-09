# SolidCraft cases triage (2026-10-24)

The `--solidcraft-cases` expected values are closed forms, not C++ results. Every
case that builds only from boxes, spheres, cylinders and Booleans was also run on the
C++ reference (scratch SM_API driver). Results on main 92454ca60:

- **3665-column-in-channel (4 ERROR):** the C++ fails too, at the same step and in the
  same function (`SmMerge::MakeFaceuse`, or `SmBrep::MakeFaceTopology` for
  `control_column_clear`), with status 1001. This is an upstream limitation, not a port
  bug (`plan/upstream-issues.md` FAIL-3665). The cases stay ERROR.
- **3396-flush-stacked-layers (5 FAIL):** Rust and C++ agree on faces, edges, vertices
  and volume (to 1e-13). The 0.01–0.16 error was the harness's integration accuracy:
  `SmApiBrepComputeVolume` at 1e-6 relative cannot meet ±0.01 on 14000–30600 mm^3. At
  1e-8 (the API's finest) both engines come within 0.0008 of the closed form.
- **1717 patterned_sphere_cut, 2844 holed plates (3 FAIL, not in the original list):**
  the driver cloned the pattern tool with `Clone`. A clone's geometry keeps the
  source's owner names, so `SmApiTranslate` moved only its vertices. The driver now uses
  `SmApiBrepCopy`. That needs two port fixes, which landed on main while this was
  open: copied faces' tolerances on their surfaces (e42e955c4) and `FaceIntersect`
  homogenizing on the lent B-reps (#809). All three now equal the C++ result.
- **2844 plain_plate_ring_sunk (UNCHECKED):** `cylinder_faces_max` was never checked.
  It now counts faces on `SmCone::IsCylinder` surfaces; the C++ reports 2 faces too.
- **Open, passing:** `1510/above_groove_4` and `above_groove_5` differ from the C++
  by about 1e-10 relative in volume, with identical topology. Everything else matches to
  the printed digits.
