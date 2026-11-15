Updates: WR-780

#### WR-780 (update, 2026-11-15). The mitre's reference step grows with the tolerance

`CreateSweepAlongPlanarPath` steps 0.1 along each leg from a mitre corner to pick the reference points that name the sides of the cutting line. WR-780 already caps that step at half the cutting line's reach. A fixed 0.1 is also no use once the tolerance passes it. The side test (`sm_FindSideOfCurve`) calls a point within the tolerance of its foot on the cutting line undecided, and the trim fails. For a circle of radius `0.1 s` along two perpendicular legs of length `s` with tolerance `1e-5 s`, every `s` from 1e4 up failed that way. Upstream ignores the failed trims and returns an invalid body of zero volume.

- **Fix (not upstream):** `step = max(0.1, 100 tol)`, still capped at half the reach. The cutting line runs through the corner along the bisector of the legs, so a point `step` along a leg lies `step * sin(beta / 2)` from it, where beta is the angle between the legs. With `step >= 100 tol` that distance exceeds `tol` whenever `sin(beta / 2) > 0.01`: every corner but a hairpin tighter than 1.15 degrees. The floor keeps upstream's 0.1 wherever `100 tol` is smaller. (ARGUED: the derivation is in the code comment; the test below checks its consequence.)
- **Tests:** `a_mitred_elbow_builds_at_any_size` in `primitive::primitive_creation::p2::planar_path`, at `s` = 1e4, 1e5 and 1e6: four faces, one manifold solid, volume `pi r^2 * 2 s` to 1e-6. It fails with the fixed step ("the point is on the curve" at `s = 1e4`).
- **Results:** no golden changes. The seven `primitive.primitive_creation_p2` elbows build, as main reported, but now as a valid solid.
