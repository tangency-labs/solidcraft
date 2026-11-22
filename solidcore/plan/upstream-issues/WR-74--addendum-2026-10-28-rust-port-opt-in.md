#### Updates: WR-74 (2026-10-28). Opt-in fix in the Rust port

- The Rust port reproduces WR-74 by default (the seven ssi-corpus cases are
  `INCOMPLETE` for C++ and the port alike, at parity). The opt-in
  `Kernel::fixes.ssi_tangent_contact_points` (`UpstreamFixes`, off by default) ports the
  SolidCraft fork's search for isolated tangential contact points
  (tangency-labs/smlib@c26edbc): the leaf pairs `SmAdvSurfaceIntersector::FindInteriorCurves`
  drops (normals within 5 degrees at the seed, no local intersection, normals within 2
  degrees at the solution) seed a Newton solve of `S1 = S2` with parallel normals (4x4,
  exact Jacobian, 5 seeds per pair and both surface roles, abandoned when it leaves the
  seed patches grown by half), and the surfaces' poles are tested directly. A root is
  added as a degenerate `TOUCHING` curve (3D and both UV) when the gap is at most
  `1e-7` times the approximation tolerance, the normals agree to `1e-4` rad and it is on
  no curve already found. Not run for a self intersection.
- **Code:** `crates/solidcore/src/intersect/adv_surface_intersector/tangent_contacts.rs`
  (`find_tangent_contact_points`), seeded from `find_interior_curves` in
  `crates/solidcore/src/intersect/adv_surface_intersector.rs`.
- **Tests:** `crates/solidcore/tests/issue_394_ssi_tangent_contact_points.rs`:
  `c3n_parab_apex`, `c3n_four_points` and `y23_t6_ex4_tangential_point` return nothing by
  default and their exact contact points with the fix.
- **ssi-corpus** (`b04c8a6`, tol `1e-6`), Rust engine with the flag on (a local run;
  `--ssi-corpus` itself always runs the default kernel): ERROR 2, INCOMPLETE 25 -> 18,
  WRONG TOPOLOGY 10, PASS 82 -> 89. Fail -> pass: all seven WR-74 cases
  (`c3_sphere_torus_outer`, `c3n_four_points`, `c3n_parab_apex`, `c3n_parab_parab_apex`,
  `y23_fig1g_three_points`, `y23_t2_ex6_loop_and_point`, `y23_t6_ex4_tangential_point`).
  No PASS case changed. One other output changed: `c7_tiny_loop_parab_1e-7` (a loop of
  diameter `2e-7`, below the tolerance) gives one touching point instead of nothing; it
  stays `INCOMPLETE`. With the flag off, parity with C++ is 119/119 identical.
- **Not covered:** higher-order contacts (equal curvature at the point), where the system
  is singular.
- tangency-labs/solidcore#394.
