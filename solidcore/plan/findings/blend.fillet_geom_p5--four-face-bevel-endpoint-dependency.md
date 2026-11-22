# Four-face bevels with both rail crossings split

Issue: SolidCraft #19143. Unit: `blend.fillet_geom_p5`.

## Reproduction and geometry

The reported three-edge, radius-2 bottom fillet fails in the installed 0.44.1 CLI with
`Fillet Corner Error: Vertex Computation Failure`. The private design is diagnostic
input only and is not part of this change.

The synthetic regression `rounds_follow_a_three_degree_crease_in_both_supports` in
`crates/solidcore/tests/wr697_ball_corner.rs` reproduces the same failure. It starts
from a 20 x 20 x 20 box, at x = 0..20, y = -10..10, z = 0..20. With
m = tan(3 degrees), four half-space cuts leave

- x <= 20 - m max(|y| - 1/2, 0),
- z >= m max(|y| - 1/2, 0).

The three selected bottom edges lie on x + z = 20. At either bend, four faces meet:
both the wall and the bottom change plane, so the two adjacent rounds share no face.
The regression checks the final solid and Level2 validity in both selection orders.
Before the fix, the targeted Rust test fails with corner error 3 and the reported
vertex-computation message.

Why the rails split can be derived without measuring an output. Translate one bend
to y = 0. The flat supports are x = 20 and z = 0; the inclined supports are
x + m y = 20 and -z + m y = 0. For radius r, the inclined spine satisfies
x + m y = 20 - r sqrt(1 + m^2) and -z + m y = -r sqrt(1 + m^2).
Its wall foot has y = y_spine + r m / sqrt(1 + m^2). On the wall crease y = 0,
that foot has z = r / sqrt(1 + m^2). The flat round reaches the same crease at z = r.
For r = 2 their separation is 2 (1 - cos(3 degrees)) = 0.00274093049085256.
The bottom crease has the corresponding separation in x. Neither pair of crossings
is one shared point.

The regression also checks the new endpoints against a closed form. Put
d = 1 - cos(3 degrees) and u = m (y - 1/2) at the positive bend. The inclined wall
rail is (20-u, 1/2+u/m, r cos(3 degrees)+u). Substitution in the flat round's cylinder
(x-(20-r))^2 + (z-r)^2 = r^2 gives
2u^2 - 2r(1+d)u + r^2 d^2 = 0. Its near root, rationalized to avoid cancellation, is
u = r d^2 / (1+d+sqrt(1+2d-d^2)). The bottom-rail end is
(20-r cos(3 degrees)-u, 1/2+u/m, u). Reflection in y supplies the negative bend's
two ends. The test compares these four points using the vertices' own tolerances.

## Cause

`FilletCorner::adjust_topology_nx2_with` in `fillet_corner/p3.rs` splits each side-edge
vertex when the two rail crossings separate. The retained vertex is processed; the
new face vertex is an unprocessed `FilletXFillet` with the smaller-side rail moved
to it. Splitting both sides leaves the central fillet/fillet edge between two new,
unprocessed vertices.

`FilletBrep::calc_fillet_int_fillet_with_context` in `fillet_geom/p5.rs` requires the
opposite endpoint already processed before it can intersect the fillet surfaces.
Both endpoints therefore depend on each other. Selection order or a different
processing order cannot resolve the cycle.

The existing WR-644 gap patch is for a three-edge corner, and Setback requires at
least three blended edges and every incident edge blended. Neither applies to this
two-blend, four-face corner.

## Construction

The rolling-ball construction's rails are the ball's feet on the supports
(`docs/generic-blend.md`, section 2.1; its Choi and Ju 1989 reference). A split end
still lies on its remaining rail and on the opposite fillet. Solve those two
rail/surface events with the existing `rail_crossing_nearest`, then intersect the
fillet surfaces between the two solved ends. The pair intersection supplies the
trimmed 3D curve and both pcurves in edge order. The ordinary rail trim recomputes
its vertex-use parameters from the solved endpoint positions.

The side-gap cut already uses the plane through the original corner, the retained
side-edge vertex, and the new endpoint. With a planar rail support, the first two
points lie on its side-edge line and the third on its rail, so this is that support's
plane exactly. The construction requires planar support faces; it does not enable
the legacy plane cut on a curved support. It uses the solvers' existing tolerances.
No retry, radius change, tolerance widening, mesh decision, or new fitted surface
is involved.

## Related report and checks

The local brief mentions the same message in #18854. Neither its design nor its issue
text is available locally, and GitHub API access failed. The executive wraps several
different vertex failures in this message, so a shared cause is not established.

Validation (all Cargo commands run through `buildq run solidcraft . --`):

- Before the fix, `cargo test --manifest-path solidcore/Cargo.toml -p solidcore
  --test wr697_ball_corner rounds_follow_a_three_degree_crease_in_both_supports
  -- --nocapture`: failed, 0 passed / 1 failed, the reported vertex-computation error.
- `cargo clippy --manifest-path solidcore/Cargo.toml -p solidcore --lib
  --test wr697_ball_corner -- -D warnings`: passed after correcting a private-helper
  lookup to use this module's existing `context.solver` API.
- `cargo fmt --all --check` and `cargo fmt --manifest-path solidcore/Cargo.toml
  --all --check`: passed on the corrected source.
- `cargo --config 'env.RUST_MIN_STACK="8388608"' test --manifest-path
  solidcore/Cargo.toml -p solidcore --test wr697_ball_corner -- --nocapture`:
  passed, 24 passed / 0 failed / 1 pre-existing ignored. The new regression passes
  both selection orders, its four closed-form endpoint checks, manifold topology,
  and Level2 validity. The stack setting matches SolidCore's own Cargo configuration.
- The CLI build was canceled before starting under the lane owner's instruction to
  finish once the fix compiles and its regression passes. The original-design replay
  on the changed kernel and the public-safe after-fix capture were skipped. The
  original-design failure on 0.44.1 was reproduced before the fix; the corrected
  behavior is verified by the synthetic regression, not by the private design.

No golden or kernel-case expectation is changed.
