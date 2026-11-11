# surface.surface_cache_p2/p3 — approximation angle arithmetic, PR #818 (2026-10-25)

The `surface.surface_point_solve_p3` `fuzz00349_rust_hang` surface has finite
homogeneous control points (one coordinate is `1e300`) but overflows segment
lengths in the sampled turning-angle walk used by its STEP/curve-bounded cache.
`compute_surf_constants` divides the segment by infinity, producing a zero
direction; its u walk can then accumulate spurious 90-degree turns. An infinite
size-derived chord tolerance does not by itself explain whether refinement is
valid, and absolute position or bounding-box magnitude is not a convergence test.

The earlier PR-head guard (`372a476d3`) was rejected during design review even
though its CI was green: it called the largest absolute endpoint coordinate an
"extent" and rejected translated measurable geometry and independent custom
testers. Its synthetic weighted boundary net did not reproduce the original
fixtures' `NOW = -f64::MAX` sentinel. Neither that cutoff nor a root exemption is
retained. The prior non-finite-tolerance guard is also absent.

The replacement records `NetConstants::nonfinite_angle_length` at the existing
angle walk's length calculation, before normalization. This is a diagnostic only:
the sampled points, formulas, operation order, branches and five numeric outputs
are unchanged. The built-in cache test returns `InvalidInput` when a surface
tested by evaluations still requests refinement after such an invalid angle
measurement. This is a numerical-failure policy, not a claim that every large
surface is divergent. Passing leaves and the original minimum-UV-size stop checks
take precedence. Exact control-net tests retain upstream behavior; the generic
traversal and custom testers impose no new coordinate or tolerance-finiteness
policy. Existing recursion and node caps, including support for the documented
2 014 255-node legitimate cache, are unchanged.

Focused library evidence: 22 tests pass (six cache-build controls, seven existing
base-test controls, nine NURBS utility tests). These include both cache
constructors on the exact p3 corpus surface, unit and `1e8` surfaces retaining
31-node trees, translated unit-side geometry at `x = 1e155` retaining the same
built-in subdivision, a custom tester splitting translated children with an
infinite chord tolerance (seven visits/nodes), and a diagnostic test preserving
the original numeric angle outputs. The separate homogeneous-coordinate
normalization smoke test is explicitly not a HANG-20 fixture substitute.

Native boundary controls remain the three unchanged
`blend.fillet_geom_p1` fixtures `fb_cliff_hang20_boundary_1e307`,
`surface_drop_point_hang20_boundary_1e307` and
`two_pnts_hang20_boundary_1e307`; the first two use the NOW sentinel, and the last
mixes sentinel and ordinary positive weights. Native comparisons and exact-head
canonical CI are separate gates, not implied by these local library tests.
No generator, fixture, validator, rejection/skip, tolerance or unit status was
changed to obtain a pass. Earlier cancelled broad local runs remain incomplete
evidence; they are not reused as successful validation.

Native macOS arm64 follow-up on the redesign (before merging main's subsequent
`25a900330` advance): the three exact boundary fixtures pass, all with values and
`max_rel_err=0`. Full affected units also pass bit-identically, zero deferred,
every case with values: `nurbs.nurbs_srf` 3092/3092,
`surface.surface_cache_p2` 905/905, `surface.surface_cache_p3` 600/600,
`surface.surface_point_solve_p1` 1035/1035, and p2 1353/1353. The combined local
command reached its 90-second budget during p3; that unit's local run is
incomplete, not a pass. It was not repeated. The optimized exact-head canonical
gate owns the full p3 comparison and whole-green-unit sweep. Reports for the
completed local units are retained in `bin/evidence/pr818-boundary` and
`bin/evidence/pr818-affected`; no full local green claim is made.
