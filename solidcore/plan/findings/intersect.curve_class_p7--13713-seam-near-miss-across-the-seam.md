# intersect.curve_class_p7: a vertex is a near miss of a seam only across it (2026-11-10)

Unit: `intersect.curve_class_p7` (`CurveClassification::has_near_misses`). Case: a dome turned
about y from a quarter disc whose arc SolidCraft drew through a point rounded to eight decimals,
so its circle has radius 10 + 1.01e-8 and its base lies 1.01e-8 above y = 0, joined flat to a
cylinder of radius 10 below it, or to its own mirror about y = 0. Both seams lie in z = 0 on +x.
Tests: `a_dome_1e_8_off_its_circle_joins_*` in `crates/solidcore-ops/tests/boolean.rs`.

**Cause.** Both unions give the closed-form volume, but `check_brep` Level 2 refused them with
face test 36 ("Face Near missing Seam"). The seam classification of each periodic face ends at
the vertex the two seams share, and `has_near_misses` counted that vertex's whole 3D gap to the
seam curve against `1000 * EFF_ZERO` = 1e-9. Measured at main 017c47a40: 1.433e-8 for the
cylinder (1.01e-8 off the dome's surface, 1.01e-8 along its seam), 2.027e-8 for the mirror (along
the seam only). Across the seam the gap is 0 in both, and the edge tolerances are 2e-5. No vertex
can lie on both seams: the cylinder's seam line crosses the dome's sphere 4.5e-4 from the circle
they share, and the mirrored seams lie on one circle but each surface stops 1.01e-8 short of
y = 0.

![The seams' plane z = 0. (a) The dome's seam ends 1.01e-8 off the surface and 1.01e-8 along the seam from the shared vertex V, which lies on the cylinder's seam: a whole gap of 1.43e-8. (b) V is the dome's seam end; the mirror's seam ends 2.03e-8 from it along the seam. Both inside the vertex tolerance 2e-5.](intersect.curve_class_p7--13713-seam-near-miss-across-the-seam/seam-gap-at-shared-vertex.svg)

*The gap at the shared vertex split into the part off the surface, along the seam and across it
(out of the page, 0 in both).*

**What test 36 is for.** It reports an edge within tolerance of the seam that is not on it
(`docs/validity-and-tolerances.md` §6): a boundary running beside the seam leaves a sliver
between them in UV, as the rounded slant plane of `issue_2538_tilted_notch_on_a_slant_edge`
does. Only a gap across the seam, within the surface, does that. A vertex off the surface or past
the seam curve's end is a gap its tolerance bounds, as in the local-tolerance model of a B-rep
(D. J. Jackson, "Boundary representation modelling with local tolerances", Proc. 3rd ACM Symp.
Solid Modeling and Applications, 1995), where an entity may lie off its geometry by up to its own
tolerance.

**Fix.** A vertex end whose gap reaches the threshold is measured again across the curve within
surface 1: the component of its offset from C(t) along n × C'(t), n the surface normal at the UV
curve 1's point. Where that direction is not defined (no UV curve 1, a vanishing tangent or
normal) the whole gap still counts. The threshold is unchanged, and a coincident edge keeps its
full gap: the rounded slant plane keeps face test 36, its ninth sliver face and edge test 9.
