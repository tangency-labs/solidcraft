# blend.blend_corner: the countersink's curved Setback is certified but a patch's control net doubles back (2026-11-03)

- **Unit:** `blend.blend_corner` (the Setback beside curved faces, #2343, PR #2366; `Kernel::fixes.fillet_setback_beside_curved_faces` with `fillet_blend_corner`).
- **Inputs:** the fork's capi case `setback curved_countersink_corner` (37c9e1d), rebuilt from solidcore calls in `crates/solidcore/tests/issue_2343_curved_setback.rs`, `countersink_corner_is_a_setback_no_more_concave_than_its_cone`.
  - The part: the block x in [-20, 4], y in [-20, 20], z in [0, 20], less a cone's frustum about (4, 0) widening at 45 degrees from radius 2 at z = 14.
  - The corner is V = (4, 8, 20): the top, the side and the concave cone. The top-side edge, the rim and the generator are rounded 1.25, 1.5 and 1. That is a convex corner beside a concave cone, k = -cos 45 / 5.5.
- **Result:** the corner is a Setback. The rule's setbacks are not certified there, so they grow to 1.414 x the rule's: 2.65125, 2.121, 3.1815 (on the thousandths grid; 1.4140625 under the 1e-2 search). The fill certifies at that scale (ruling (a), containment), and the fillet completes.
  - `check_brep` level 2 then reports: "SmBSplineSurface::Test #5 Bad NonMonotonic CPts: SmBSplineSurface Control Net doubles back on itself".
- **Also:** the D prism (`d_prism_case`) asked for the setback its growth named for 3.05 under the thousandths search before the fix, 3.23605 (`the_named_setback_builds`). That fill was certified at the setbacks themselves, and two of its patches' nets failed test #5.
- **Expected:** a sound body (17 faces, 2 n = 6 patches).
- **Same as the fork:** its open item (b) at 37c9e1d: "grown to 1.414 x the rule's setbacks the fill certifies, but a patch's control net doubles back (SmBSplineSurface test #5 in sound())".
- **Suspected cause:** the certificate bounds the surface (`II >= k G` and its normal one-signed), not its control net. Test #5 checks the net. A patch can be regular and certified while its net doubles back.
  - It is open whether the fill should keep its nets monotone (a constraint on the quadratic programme) or the net test is too strict for a certified patch. That is a design question.
- **Cause (measured by jyviko on #2243, and here):** the doubling back is real, not rounding (not #2560): steps of 0.065 mm and up, turns of 170 to 177 degrees, the nets up to 1.8 mm past a plane face while the surfaces stay on the faces. The fill's programme was all about the surface: the thin-plate energy at 8 x 8 Gauss points does not see a high-degree net that rings between them, and the degree search raised the degree (to 9) until such a net certified.
- **Resolution (2026-11-03, the owner's answers on #2243, solidcore-native under the same flag):**
  - the programme also minimises the net's own thin-plate energy (its degree-scaled second differences);
  - a patch is certified only with a net that never turns back (`check_brep`'s test #5) and that is contained: past a plane by at most the tolerance, past a curved face by at most how far its tangent planes leave it within the net;
  - with them every case certifies at degree 5 (before: up to 9). The countersink's rule grows to 1.416 x (2.655, 2.124, 3.186) and builds, sound and graded; asked 3.2 it builds as it is.
- **State:** resolved. `countersink_corner_is_a_setback_no_more_concave_than_its_cone` (the grown rule; ignored as slow) and `countersink_asked_corner_is_certified_with_its_nets` (not ignored) pin every patch's net with test #5, and so does every case's grading.
- **The convex cone** (the fork's `curved_cone_corner`, known failing in the fork at 37c9e1d) passes in solidcore: `cone_frustum_corner_is_a_setback_beside_the_cone`, at the rule's setbacks 1.875, 2.25 and 3.
