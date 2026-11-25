# Minimum distance between selected trimmed faces

Issue #19061 reproduces with two concentric semicircular arcs of radii 20 and 35,
extruded 5 mm. The face pair measured 14.979934139493198 mm. Its first witness,
(14.834599687613975, -13.374049075857705, 2.5), belongs to a display triangle,
inside the radius-20 cylinder. The exact dispatch handled edges but omitted face pairs.

The selected-shape solve uses the existing topology solver's ordered vertex, edge
and face comparisons. Each shape includes its faces' boundary edges and vertices.
The query evaluates both witnesses on the solution's named geometry, with the
parameter offset belonging to that entity kind. It introduces no query tolerance.

Ops copies both operands and gathers their topology into one private measurement
B-rep without intersection or analytic reclassification. It composes the original
to copy and source to merged maps for each selected face. The temporary B-rep is
not tested as a combined solid: measuring overlapping bodies is valid. The original
bodies and their record orders are unchanged. Face-pair solve errors propagate to
the command rather than substituting display triangles.

The tests derive their expected distances independently:

- Concentric cylinder walls: 35 - 20 = 15 mm, with witnesses at their exact radii
  (their squared XY norms are 20² = 400 and 35² = 1225).
- Spheres centred at (0, 0, 0) and (12, 16, 0), of radii 5 and 8:
  sqrt(12² + 16²) - 5 - 8 = 7 mm. Along the line of centres, the witnesses are
  (3, 4, 0) and (7.2, 9.6, 0).
- Trimmed unit-box faces nearest at (1, 1, 1) and (3, 4, 4):
  sqrt(2² + 3² + 3²) = sqrt(22) mm, rather than their planes' 3 mm gap.
  The test checks the equivalent squared-distance identity, distance² = 22.
- Opposite faces of a 1 × 2 × 3 box: 3 mm. Adjacent faces: 0 mm at their boundary.

No existing golden or kernel-case expectation is changed.
