Updates: WR-713

#### WR-713 (update, 2026-11-17). `cone_unbounded_parameterized_as_nurb` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, #18160, together with WR-704's `cone_iso_circle_at_axial_height`). Removed: the field `UpstreamFixes::cone_unbounded_parameterized_as_nurb`, SolidCraft's preset line, its entry in `fixes.json`, and every test setting of it.

Running with the fix always on exposed further defects in the same constructors. They are fixed at the root (`surface/cone/p1.rs`, `surface/cone/p2.rs`):

- **The canonical frame.** `Cone::canonical_frame` takes tiny and huge axes as directions. `SetCanonical` unitizes an axis with a coordinate of 1e300 to zero and still reports success, so a frame built that way is checked and refused.
- **`Cone::new_unbounded`** refuses inputs that define no cone, each decided in closed form from the input numbers:
  - a non-finite origin or radius;
  - a semi-angle of 90 degrees modulo 180, or one that is not finite: `tan` has no value there, and the generator perpendicular to the axis sweeps a plane, every point at height 0;
  - a zero or non-finite axis: it has no direction, so there is no axis `Z`.

  A semi-angle outside (−90, 90) is taken modulo 180 into it. `s` and `s ∓ 180` have the same tangent and the same generator line, so they are the same cone; the class documents its angle between −90 and 90. Upstream kept such an angle, and a semi-angle of 120 ran its domain from the apex away from the base circle.
- **The unbounded generator.**
  - It is built in the canonical frame.
  - It is rotated as a vector for every cone that is not a cylinder. Upstream rotates the point `origin + Z` and then subtracts `origin`, and at an origin around 1e5 that rotation is lost in rounding.
  - The apex end of the domain is clamped to `SM_INFINITE_PARAMETER`.
  - `transform` resets the generator's speed and domain when it scales.
- **`Cone::new_bounded`** takes its generator axes from the canonical frame. With axes that are not orthonormal, upstream put the generator at `origin + radius X`, which gives a cylinder of radius `radius |X|`.
- **`Cone::create_offset_surface`.**
  - The kept cone of a truncated offset runs from the cone's own STEP heights. Upstream placed it as if the bottom were at the origin, which for an unbounded cone gives another cone.
  - A copied offset rebuilds its polar circle on the offset cone. Upstream kept the original's circle, which is off the offset by the offset distance.

- **Results:** 4,625 goldens rows move (against main after the WR-662 retirement, #18066):

  | Unit | Rows |
  |---|---|
  | `surface.cone_p1` | 3,622 |
  | `surface.surface_p2` | 941 |
  | `surface.cone_p2` | 27 |
  | `surface.cone_p3` | 14 |
  | `intersect.api_intersectors` | 8 |
  | `nurbs.b_spline_surface_n_lib_p2` | 6 |
  | `primitive.api_primitives` | 6 |
  | `surface.surf_of_extrusion_p2` | 1 |

  `crates/goldens/tests/wr704_wr713_cone_moves.rs` (ignored; it runs on the inventory of moved rows) certifies every row by closed form or certificate, with no sampling:
  - 4,152 rows are CLOSER: the new output meets the closed form and the old one does not.
  - 473 rows are EQUAL: both outputs meet it.
  - No row is FARTHER.
  
  The certificates:
  - Built cones are checked net by net with `implicit::lies_on`.
  - Intersections of cylinders, of a cylinder and a cone, and of a cylinder and a plane are checked against their closed-form lines or emptiness.
  - The corner blends are checked side by side against their loop curves, by Bézier blossoms.
  - The extrusion and cone curves are checked by convex-hull plane distances and by Bernstein bounds of the implicit cone.
- **Tests:**
  - `crates/solidcore/tests/issue_2313_unbounded_cone_iso_circle.rs` asserts the fixed behaviour only.
  - `crates/solidcore/tests/wr713_cone_root_fixes.rs` has a test for each root fix above against its closed form. Each was checked to fail with its fix reverted, except the refusals, which `unbounded_cone_refuses_inputs_that_define_no_cone` asserts directly.
