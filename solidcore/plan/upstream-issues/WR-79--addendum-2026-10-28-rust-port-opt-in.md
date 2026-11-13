#### Updates: WR-79 (2026-10-28). Part 1 (sphere/plane, sphere/sphere) ported as an opt-in fix

- **Cause traced (analytic path, tangency-labs/solidcore#396 part 1):**
  `SmSphere::IntersectWithPlane` returns one `TOUCHING` point when
  `dDistToPlane > m_dRadius - dTol`, and `SmSphere::IntersectWithSphere` when
  `dDist + dTol > r1 + r2` or `dDist - dTol < |r1 - r2|`. Both compare the gap to
  tangency with the tolerance, but the circle's radius is `sqrt(2 R gap)`, so at
  `dTol = 1e-6` and `R = 1` circles of radius up to about 1.4e-3 collapse to a point at
  their centre (`c7_tiny_loop_sphere_1e-4`, `_1e-5`).
- **Rust port:** `Kernel::fixes.sphere_small_circle_not_touching` (`UpstreamFixes`, off by
  default; `crates/solidcore/src/surface/sphere.rs`). With it on, the point is returned
  only when the circle's own radius is within the tolerance:
  `gap <= 0 || gap * (R + d) <= tol^2` for the plane, and for two spheres
  `(d+r1+r2)(r1+r2-d)(d-r1+r2)(d+r1-r2) <= 4 tol^2 d^2` (each factor formed from the
  gaps directly, so it stays accurate near tangency), choosing inside contact when the
  inner gap is the smaller. This is the SolidCraft fork's tangency-labs/smlib@30d25bf
  (`SmSphere.cpp` part).
- **Kind:** a result-changing fix (a wrong result, no crash, undefined behaviour, hang
  or nondeterminism), so it follows the opt-in rule; off by default the port matches the
  pinned C++.
- **Test:** `crates/solidcore/tests/issue_396_small_loops.rs` (circles of radius 1e-3,
  1e-4, 1e-5 and 1e-7 at tolerance 1e-6, flag off and on, plane and sphere).
- **Not covered:** part 2, the NURBS paraboloid/plane cases (`c7_tiny_loop_parab_*`),
  where `SmAdvSurfaceIntersector::FindInteriorCurves` drops low-angle start points; it
  stays open under #396.
