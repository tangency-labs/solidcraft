#### Updates: WR-75 (2026-10-31). Tangential-contact corpus and the issue's cases with the tracer

- **Difftest corpus (issue #395):** `fixtures/surface.surface_p2/ssi_tangential_contact.json`
  (`harness/fixturegen/ssi_tangential_contact.py`). It has 28 `gssi` cases: 14 pairs of
  exact NURBS surfaces at the corpus tolerance 1e-6, each in both operand orders.
  - The rational cylinders, spheres and tori are 9-point rational circles, swept or
    revolved.
  - The polynomial graphs are Bezier patches with the exact Bernstein coefficients.
  - Each pair touches along a known line, circle or point.

  All 28 are at parity with the pinned C++.
- **What upstream returns** (C++ and the default Rust kernel alike):
  - **Traced:** the second-order line and circle contacts. These are a cylinder on a plane,
    `z = x^2` on a plane, parallel cylinders touching outside and inside, a torus on a
    plane, and a sphere in a cylinder. Each comes back as one `TANGENT` curve.
  - **Missed:** the closed tangent circle of `z = (x^2 + y^2 - 1)^2` on a plane. Nothing
    comes back.
  - **Collapsed:** the 3rd-order (crossing) and 4th-order (touching) tangent lines
    `z = x^3` and `z = x^4` on a plane. Each comes back as two `TOUCHING` points at its
    ends.
  - **Isolated point contacts** (a sphere or paraboloid on a plane): nothing comes back.
    This is WR-74, not WR-75.
  - **Crossing nodes:** the Steinmetz cylinders, the saddle on a plane and the crossing
    parabolic cylinders come back as complete crossing curves. The tangent nodes are not
    reported as points.
- **With `Kernel::fixes.ssi_tangent_curve_tracer`**
  (`crates/solidcore/tests/issue_395_tangential_contact_corpus.rs`):
  - Every contact curve of the corpus is covered end to end, in both orders. Every
    returned curve lies on the exact contact within 1e-4, which is the closed form's
    sampling.
  - The missed circle and the collapsed lines each come back as one `TANGENT` curve.
  - The second-order contacts are unchanged.
  - The Steinmetz nodes `(0, +-1, 0)` are reported as `TOUCHING` points.
  - The isolated point contacts still need `ssi_tangent_contact_points` (WR-74).
- **The issue's ssi-corpus cases** (`b04c8a6`, tol 1e-6). These are local runs: the Rust
  engine was patched to turn the flag on, and `--ssi-corpus` itself always runs the
  default kernel. With the flag off, all 17 cases run are identical to the C++. With the
  tracer, these go from fail to PASS:
  - `y23_t2_ex1_tangent_line` (INCOMPLETE)
  - `y23_t2_ex2_tangential_curve` (INCOMPLETE)
  - `y23_t2_ex3_loop_two_curves` (INCOMPLETE)
  - `y23_t2_ex4_tangential_curve_saddle` (WRONG TOPOLOGY)
  - `y23_fig1d_two_quads` (INCOMPLETE)
  - `y23_fig1f_high_order_contact` (INCOMPLETE)
  - `y23_t6_ex6_four_boundary_curves` (INCOMPLETE)
  - `c2n_quartic_circle` (INCOMPLETE)

  The other cases:
  - `y23_fig1c_boundary_triangle`: the output changes, and it stays PASS.
  - Unchanged: `y23_fig1a` (WRONG TOPOLOGY, singular points; WR-77), `y23_fig1b`,
    `y23_fig1e` and `c4_steinmetz` (PASS at this corpus commit), and `y23_fig1g` and
    `y23_t2_ex6` (isolated points; WR-74).
  - `y23_t2_ex5_two_loops` and `c2n_quartic_contact_parabola` stay ERROR
    (`SM_ERR_NULL_POINTER`, WR-80) with the tracer alone.
  - With every merged SSI opt-in on, `c2n_quartic_contact_parabola` goes to PASS, and
    `y23_t2_ex5_two_loops` goes to WRONG TOPOLOGY. The merged opt-ins are
    `ssi_tangent_curve_tracer`, `ssi_tangent_contact_points`,
    `ssi_trace_tangent_curves_once`, `ssi_no_reverse_of_empty_trace`,
    `ssi_singular_start_inside_direction` and `ssi_pole_point_keeps_its_tangent`. The
    issue expects `y23_t2_ex5_two_loops` to pass only with all the fork's SSI fixes, and
    WR-77 and WR-79's probation are still open PRs.
- tangency-labs/solidcore#395.
