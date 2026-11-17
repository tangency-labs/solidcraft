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

- **Results:** 4,545 goldens rows move (against main after the WR-662 retirement, #18066):

  | Unit | Rows |
  |---|---|
  | `surface.cone_p1` | 3,561 |
  | `surface.surface_p2` | 899 |
  | `surface.cone_p2` | 27 |
  | `surface.cone_p3` | 14 |
  | `intersect.api_intersectors` | 8 |
  | `nurbs.b_spline_surface_n_lib_p2` | 28 |
  | `primitive.api_primitives` | 6 |
  | `surface.surf_of_extrusion_p2` | 1 |
  | `topology.brep_p1` | 1 |

  `crates/goldens/tests/wr704_wr713_cone_moves.rs` (ignored; it runs on the inventory of moved rows) certifies every row by closed form or certificate, with no sampling:
  - 4,035 rows are CLOSER: the new output meets the closed form and the old one does not.
  - 508 rows are EQUAL: both outputs meet it.
  - 1 row is FARTHER: `nurbs.b_spline_surface_n_lib_p2` analytic064 (below).

  179 rows change a built cone into a refusal, each for inputs that define no cone, decided in closed form from the input numbers: 165 unbounded cones with a semi-angle of exactly 90 degrees (a plane), 13 with a zero axis (no direction), and one cylinder of radius 0 (a line). Two `surface.cone_p2` trims refuse where main succeeded; they rest on reading an axis of `(0, -1e-300, 0)` as the direction `-Y`, a convention (main replaced it by `Z`). That convention, with reading a huge axis `(0, -1e300, 1)` as its direction and an axis of `5e-13` (below upstream's unitize threshold) likewise, carries 32 moved rows in all (2 `cone_p1`, 16 `cone_p2`, 14 `cone_p3`), listed in the proof's module documentation; they are argued, not proven. The 28 `nurbs.b_spline_surface_n_lib_p2` corner blends are certified for G0, and for G1 by the sup-norm angle on each side against the neighbour's implicit normal. That sup is corner-saturated (the shared loop curves fix it at the corners), so EQUAL there says the corners agree, not the side interiors. 12 moved because the neighbour did; 16 because the Coons refinement in `make_coons_patch_tangent_to_sides` no longer decides by rounding: per-side maxima within the a-priori rounding bound of `coons_side_angle` are tied, and a tie takes the finer refinement (deterministic), and the side angle is sampled per knot span so graded sliver spans are not stepped over. One row is FARTHER: analytic064 (torus), by 5.2e-5 in `sin` inside the sliver span next to a corner. `crates/goldens/tests/wr713_coons_tie_finer.rs` checks analytic028 takes the refined patch. The tie rule also moves one `topology.brep_p1` row (`vase_rev_corner_blend_3`, a corner blend on surface-of-revolution faces, which take a finer patch); its neighbours have no implicit form and it is not certified.
  
  The certificates:
  - Built cones are checked net by net with `implicit::lies_on`.
  - Intersections of cylinders, of a cylinder and a cone, and of a cylinder and a plane are checked against their closed-form lines or emptiness.
  - The corner blends are checked side by side against their loop curves, by Bézier blossoms, and the curves on their cylinder.
  - The extrusion and cone curves are checked by convex-hull plane distances and by Bernstein bounds of the implicit cone.
- **Tests:**
  - `crates/solidcore/tests/issue_2313_unbounded_cone_iso_circle.rs` asserts the fixed behaviour only.
  - `crates/solidcore/tests/wr713_cone_root_fixes.rs` has a test for each root fix above against its closed form. Each was checked to fail with its fix reverted, except the refusals, which `unbounded_cone_refuses_inputs_that_define_no_cone` asserts directly.
