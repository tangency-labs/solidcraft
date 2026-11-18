Updates: WR-75

#### WR-75 (update, 2026-11-18). A self intersection over overlapping domains runs no tangential curve pass (#18930)

Not the fork's; a change to the opt-in `Kernel::fixes.ssi_tangent_curve_tracer` itself.

- **What happens:** the tracer solves `S1(u1, v1) - S2(u2, v2) = 0` with `n1 = +-n2` (Levenberg-Marquardt,
  `tangent_tracer.rs`). In a self intersection `S1` and `S2` are one surface, so every point
  solves it with itself at equal parameters: the whole diagonal `(u1, v1) = (u2, v2)` is a
  solution. That trivial solution is what a self intersection has to leave out (Pekerman, Elber
  and Kim, "Self-intersection detection and elimination in freeform curves and surfaces",
  Computer-Aided Design 40 (2008)); the interior search leaves it out by intersecting only
  disjoint patches (`AdvSurfaceIntersector::find_interior_curves`), but the tracer's solves are
  free to move both points. Where a surface folds back on itself (a cusp, as an offset past its
  radius of curvature has), its two sheets meet at equal parameters, the patches beside the
  fold have nearly parallel normals and seed the pass, and the solves slide onto the diagonal.
  The pass then traces the surface against itself, in "tangent curves" on the diagonal.
- **Trigger:** the goldens' `offset.offset_geometry_creation` gen0154, gen0495 and gen0696,
  each a surface of revolution asked for its self intersection (gen0154's: a full turn of a
  generatrix of 1,445 control points). With the flag on, gen0154's one pass
  took 89,337 seeds, solved 2,981 contacts on the diagonal and traced 1,472 tangent curves
  (302 k points, 1.42 M after span refinement, 12.6 M solves) and left 588 curves. The three
  cases took 1,510 of the 1,537 s the pass spent over the unit's 926 cases, and with the flag
  on 15 of the unit's goldens rows moved.
- **Change:** a self intersection whose two domains overlap or touch runs no tangential curve
  pass and collects no seeds for it (`AdvSurfaceIntersector::trivial_solution_in_both_domains`),
  as a self intersection already runs no boundary start point search, no tangential contact
  point search (WR-74) and no singular point splits (WR-77). Over two disjoint domains of one
  surface the pass runs: the tracer keeps each solve in its own domain, where no point has
  its own parameters in the other.
- **Limit:** a surface touching itself along a curve at two different parameters inside one
  domain is left to the marcher, as without the flag. The extrusion of
  `c(u) = (u^2, u (u^2 - 1)^2)` over `u` in [-1.5, 1.5] touches itself along the line where
  `c(-1) = c(1) = (1, 0)`: over its whole domain the pass traced that line as one tangent curve,
  and now, as without the flag, nothing is returned. Finding such a curve with the pass needs
  the trivial solution taken out of the tracer's equations themselves, not done here.
- **Test:** `intersect::surface_intersector::p5::tests::a_folded_ribbon_crosses_itself_once_with_the_tangent_curve_tracer`:
  a vertical segment swept along the offset by 2 of `y = x^2 / 2` towards its centres of
  curvature (a swallowtail). With the flag on, its self intersection was the crossing line at
  (0, 2.5) and six tangent curves; it is now the crossing line alone over the ribbon's height,
  as without the flag.
  `..._meets_its_trivial_solution_where_its_domains_overlap_or_touch`: the same, overlapping
  and touching domains of one surface skip the pass; disjoint ones and two surfaces run it.

![Left: the folded ribbon seen from above, its path crossing itself once and turning back at two cusps. Right: the square of pairs of path parameters (t1, t2); the crossing is a pair off the diagonal t1 = t2, the cusps lie on the diagonal, where every point solves the tracer's equations](WR-75--addendum-2026-11-18-no-tangential-pass-in-a-self-intersection.svg)

*Look at the cusps: they lie on the diagonal of the parameter square, where every pair is a
solution; a solve seeded beside a fold can end there.*
