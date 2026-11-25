#### Updates: WR-79 (2026-11-05). Part 1 is the only path; a cap within the tolerance is a touch (#2412)

- **Retired flag:** `sphere_small_circle_not_touching` is deleted. `SmSphere::IntersectWithPlane`
  and `IntersectWithSphere` now always collapse the circle to a touching point only when its
  own radius is within the tolerance (the rule of `WR-79--addendum-2026-10-28-rust-port-opt-in.md`).
  Upstream's test (the gap to tangency within the tolerance) is gone.
- **What went wrong with the circle (#2412):** a 10^3 box and a radius-3 sphere crossing its
  top face by `gap` gave a wrong body while the circle was a crossing: box − sphere had volume
  0 at gaps 1e-6 to 1e-9 (open at 1e-7, check failure `SmTrimmingTools` test 1 at 1e-9), box +
  sphere at 1e-9 was open with volume 1000, and at 1e-11 both failed. The merge's SSI
  tolerance there is 5e-6, its edge tolerances 1e-5. The faces cross at `θ ≈ sqrt(2 gap / R)`,
  0.002 to 0.05 degrees, so they stay within the tolerance of each other across a band
  `tol / θ` wide round the circle, 25 to 2500 times the circle's own radius. `SmEdge::
  RadialSectorClassify` pairs faces under 0.5 degrees as coincident: the box's top face with
  the sphere's lower face, and the disc with the cap. Step 7 then flags every region void, or
  the cap's seam is left as a slit.
- **The most correct body.** The exact solid is the box with the sphere's cavity opened
  through a hole of radius `sqrt(2 R gap)`; its volume is `1000 ∓ 36π ± π gap² (3R − gap) / 3`.
  Inside the hole the exact boundary (the cap) is `gap` from the face, within the tolerance;
  outside it the sphere and the face part by less than the tolerance across the band above. A
  B-rep at that tolerance cannot hold an edge there: the sliver between the faces is thinner
  than the tolerance. The body that is right at the tolerance is the touch: box and sphere
  meeting at the cap's centre. It is within `gap` of the exact boundary everywhere and its
  volume differs from the exact one by the cap's, under 1.6e-10 for every gap at or below the
  tolerance. With upstream's test that is what the boolean built, and it is what it builds now.
- **Fix (in the B-rep, not the intersector):** `FaceIntersect` (`topology/face/p3.rs`,
  `cap_centre_within_tolerance`) takes a crossing curve whose cap lies within the
  approximation tolerance as a touching point at the cap's centre. The test is local and
  general: to second order the separation of two surfaces near a contact is a quadratic whose
  zero set is the curve; at a curve point the crossing angle `θ` over twice the curve's
  curvature `κ` is the cap height for a circle (`r² / 2R` for a sphere cut in a circle of radius
  `r`), and for an elliptic level set the height at the centre is the geometric mean of that
  quotient at the axis ends, so no more than its largest value. The curve is a touch when the
  quotient is within the tolerance at all eight samples and the mean centre of curvature (the
  circle's centre) lies within the tolerance of both surfaces, as a point of contact must; a
  tangent curve, where the quotient is near zero too, has its centre of curvature far from
  both. The point is made once for the pieces of one cap. So the SSI answers the
  exact circle as a query (ssi-corpus `c7_tiny_loop_sphere_*`) and the boolean builds the body
  that is right at its tolerance.
- **Results:** the box and sphere at gaps 1e-5, 4e-6, 1e-6, 1e-7, 1e-8, 1e-9 and 1e-11, both
  operations: closed manifold solids, seven faces, check clean, volumes within 1e-8 relative
  of the closed form (integrator accuracy 1e-7). At every gap from 4e-6 down the body has the
  previous default path's (upstream's test) topology and volume to the ten digits printed; at
  1e-5 and 6e-6 the circle stays a crossing edge in both. Test: `box_and_sphere_poking_a_hair_through_a_face_is_a_closed_solid` in
  `crates/solidcore/tests/issue_396_small_loops.rs`.
- **Moved difftest cases:** `surface.sphere`'s `IsectPlane`, `IsectSphere` and `Global` cases
  whose gap to tangency is within the tolerance while the circle's radius is not now return the
  circle where the pinned C++ returns a point. The pinned C++ is wrong for those cases (this
  record's base entry is confirmed by the C++ build), so each is a `cpp_defect = "WR-79"`
  mismatch, to be named in an addendum from a difftest run against the C++ reference (not
  available on the host this was written on). `FaceIntersect` changes a result only for a
  crossing curve whose cap is within the tolerance, which upstream's sphere test already
  collapsed in the intersector.
- **Two spheres:** radius-3 spheres overlapping by 1e-4 to 1e-10 unite into the same bodies as
  the old default (closed, volume 72π to 3e-8 from 1e-6 down; at 1e-6 to 1e-8 the check notes
  `SmFace` #36, an edge near the seam, as before). **Not fixed here:** at an overlap of 1e-5 (a
  circle above the tolerance, crossing at 0.2 degrees) the union is open with half the volume,
  before and after this change: the same 0.5-degree pairing in `RadialSectorClassify`, outside
  part 1's band.
