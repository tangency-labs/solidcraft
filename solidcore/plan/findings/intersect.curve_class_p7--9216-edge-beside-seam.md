# intersect.curve_class_p7: an edge beside a seam bounds a sliver only with a face of the seam's edge (2026-11-15)

Unit: `intersect.curve_class_p7` (`CurveClassification::edge_runs_beside`, called by
`has_near_misses`). Case: SolidCraft #9216, the reporter's Extrude3, a plate with a ring and a
lug whose profile has three holes touching at one point. Test:
`holes_touching_at_a_tangent_point_extrude_to_a_valid_solid` in
`crates/solidcore-ops/tests/sweep.rs`.

**Cause.** At (0, -60) the line y = -60 is tangent to the r 60 circle and to the r 3.5 circle of
the lug. The two slits between the line and the r 60 circle and the r 3.5 hole meet there; the
r 3.5 hole is one loop with the slit it touches, so its wall is one full cylinder whose seam edge
S stands at that point. The sketch solver left the r 3.5 circle's centre 2.6e-8 off x = 0, so the
shared vertex V (placed from the first slit's curves) lies 2.6e-8 across the seam, inside its
2e-5 tolerance. Measured at main 7e0c36cf83: the extrusion's face test 36 fails on that vertex.
#14849 (#13295) bounds such a vertex's gap by its tolerance, unless another edge at the vertex
runs beside the seam. Here two do: the tips of the two slits, the edges where each slit's r 60
wall meets its flat wall, 0 and 2.6e-8 from the seam. They bound only the slits' own walls.

![Plan view of the touching point, not to scale: the r 60 circle and the line y = -60 meet at the vertex V; the r 3.5 circle touches the line at S, its seam edge seen end-on, 2.6e-8 from V and inside V's tolerance zone; slit B lies left of V between the circle and the line, slit C right of S.](intersect.curve_class_p7--9216-edge-beside-seam/edge-beside-seam.svg)

*The vertex V, the seam edge S of the r 3.5 wall, and the slits whose tips stand beside S.*

**What test 36 is for.** A sliver: a face between an edge and the seam, a tolerance wide
(`docs/validity-and-tolerances.md` §6, #14848). Such a face has both the edge and one of the
seam's own edges on its boundary. In the local-tolerance model (D. J. Jackson, "Boundary
representation modelling with local tolerances", Proc. 3rd ACM Symp. Solid Modeling and
Applications, 1995) a vertex inside its tolerance zone lies on every edge it ends, so the gap at
V alone is no defect; only a face squeezed between two edges within that zone is.

**Change.** `edge_runs_beside` counts an edge only when it bounds a face that one of the seam's
own edges bounds. Every edge of the checked face still counts (the seam's edges bound it), and
so does every edge of a face across the seam's edges. Where the own edges' faces cannot be
resolved, every edge counts, as before. The extrusion is one valid solid: 24 faces, 66 edges,
40 vertices, one shell, 1127.958 mm³ (3 × the profile's area by Green's theorem, relative
3.9e-10).

**Not covered.** A real sliver beside a seam is still not in the repo as a case (#14848).
