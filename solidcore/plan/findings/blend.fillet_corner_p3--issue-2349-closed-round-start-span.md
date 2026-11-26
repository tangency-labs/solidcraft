# blend.fillet_corner_p3: a closed round's first span from an angled tip is off the true round (2026-11-05)

Unit: the closed Turn (#2349) shows it; the defect is in the round itself, from its trace
(fillet solver and section), not in the corner. It is on main before #2349's change: the
change keeps every filleted teardrop bit for bit (vertices and edge samples at full
precision, radii 0.5, 1, 1.5 and 3, default and generic-blend kernels).

## Inputs

The #2349 teardrop (`crates/solidcore/tests/issue_2349_closed_angle.rs`): the cubic with
poles (0,0), (20,-10), (20,10), (0,0), extruded 10 along z. Its top rim is one closed edge
meeting itself at the tip (0, 0, 10) at an angle. The rim rounded at r 0.5 builds five faces
and passes `check_brep` Level2.

## Closed forms

The round between the top plane z = 10 and the vertical wall over the profile γ is, in
each plane square to γ at t, the ball's circle: a point at height z is on it where its
distance to γ is r (1 - cos a), sin a = (z - 10 + r) / r. A section's points move along
γ'(t) exactly (d/dt of γ - d n, n the profile normal, is γ' (1 + d κ)). The teardrop is
symmetric about y = 0, so the round's two ends are mirror images and its mitre lies on
y = 0. Measured with `distance_to_profile` over each half of the profile in the test file.

## Measured (generic-blend kernel, every fix but `geometric_g2_continuity`; r 0.5)

- The side vertex is (0, 0, 9.5) exactly; the top vertex is (1.1473, -2.6e-6, 10).
- Face 6, the round's end over t in [1/2, 1], is on the true round to 1e-7 up to the tip.
- Face 5, the end over t in [0, 1/2] (surface u from 8.5323 at the tip to 24.3476), is
  exact at the tip row (1.6e-14) and on its rails, but its first span is off the true round
  between them: 2.0e-4 at u 8.61, 6.0e-4 at 8.85, 7.9e-4 at 9.16, 4.3e-4 at 9.80, 1.9e-4 at
  11.06, 2.0e-6 at 13.28 and after.
- Its start derivative is tilted between the rails: unit d/du at the tip is
  (0.895614, -0.444831) at v 0.5 and (0.895279, -0.445505) at v 0.25 and 0.75, where every
  section point moves along γ'(0) = (0.894427, -0.447214) (as the rails at v 0 and 1 do):
  about 2.7e-3 rad.
- The mitre lies on both faces' surfaces to 1.2e-5, and on face 6's end of the true round to
  2.7e-5, but it is up to 6.6e-4 off face 5's end and 3.7e-4 off y = 0; its edge tolerance
  is 4e-5. The default kernel (polynomial sections, WR-708) gives 1.8e-3 and 1.05e-3.

## Suspected cause

The first station's derivative rows (the Hermite span's start) at a closed edge's start:
a derivative taken across the closure, as if the edge were G1 there, would tilt it toward
the other end's tangent while the rails, built separately, stay exact. Not yet traced.

## Consequence

The closed Turn's mitre is the intersection of the two ends' surfaces and inherits the
error; it is recorded as the known-failing test `closed_mitre_is_on_both_ends_of_the_round`.
