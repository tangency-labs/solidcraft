# Planar chamfer bands: size the original curves' contact

Issue: SolidCraft #19108. Addendum to
`blend.fillet_executive--narrow-curved-tip-chamfer-refusal.md`.

The refusal remains: the requested chamfers overlap on the planar top face. The
diagnostic now sizes that contact and identifies the limiting face.

## Construction and proof

A chamfer between a plane and another plane has a constant setback on the first
plane. The same holds between a plane and an extrusion whose direction is normal
to the plane: the shared wedge is a right angle all along its curved edge. The
support definitions select this class before solving; a failed solve never invokes
the old station search.

The application exports native curved extrusion walls as B-splines at the design
record boundary. A degree-one, two-control-point sweep direction is recognized
directly from that stored homogeneous net: paired control points have identical
positive weights (or both are non-rational), and every pair has exactly the same
Euclidean translation. Error-free expansions compare weighted differences without
rounding a residual to zero. The translation must be normal to the shared plane,
using the same arithmetic-zero test as a native extrusion. This proves the same
support class; all subsequent evaluations still use the original B-spline. No
fitted analytic surface or sampled recognizer is constructed. The ops regression
round-trips the native outline through the record to exercise this boundary.

For two such bands, let their inward widths on the plane be a and b. The kernel's
existing bounded curve/curve minimum query gives their original edges' closest
points P and Q, including endpoints, and separation W. If Q-P follows the first
inward normal and opposes the second, their inward normal rays meet at

    lambda = W / (a + b)
    C = P + lambda a n_P = Q + lambda b n_Q.

No earlier contact is possible: the triangle inequality would then give an edge
separation below W. These opposing normal witnesses attain that bound. The contact
must lie on the trimmed common face. At an endpoint, the construction additionally
requires an unselected straight cap edge whose other face is a plane containing
the inward normal ray. A selected adjacent edge instead places a moving corner,
so that endpoint does not establish this event's kept domain.

An interior original-edge foot is not automatically a kept witness: a selected
neighbor at either endpoint can trim past it. For every such neighbor, require a
constant planar chamfer with a native straight edge on the same shared plane.
At the closed candidate scale, the contact must lie strictly outside each of those
removed strips. An infinite-line distance is a lower bound for the bounded-edge
distance, so it supplies a sufficient certificate. For original feet P and Q,
widths a and b, neighbor line point L, line vector T, and neighbor width r,
eliminate the common denominator a+b from the contact and scale:

    |b ((P-L) x T) + a ((Q-L) x T)|^2 > |Q-P|^2 r^2 |T|^2.

The expansion arithmetic forms each cross-product difference, weighted sum and
square exactly. Neither a rounded contact point nor a rounded scale enters this
comparison; it requires no square roots or new tolerance.
Out-of-range arithmetic returns an error. A contact outside the incident planar
strips cannot be cut away by their planar mitres. An unsupported neighbor or a
contact without that clearance names no fit; there is no sampled replacement.
The fixed-cap endpoint certificate above remains independent of this interior test.

This uses the existing global minimum query on the support curves, not a new
sampled minimum, a mesh, or a search that rebuilds candidate solids. It does not
claim to add a new interval certificate to that existing query. The normal tests
use the kernel's arithmetic zero; the open margin uses the edges' own tolerances.
For native parallel lines, the closest set is the overlap of their bounded
projections on their common tangent. Its midpoint supplies a relative-interior
candidate, rather than relying on a single minimum query's endpoint tie ordering;
it must also pass the retained-witness certificates above.
Disjoint projections cannot supply opposing normal witnesses. This represents the
same minimum-gap contact, and covers the previous exact parallel-line event.
The native line directions' cross product must be zero by the existing error-free
expansion arithmetic; a small angle is not a minimizing family. The recovered
parameters are evaluated on each original curve, preserving both curve definitions.

One call whose offsets all equal one common radius retains the existing closed
Radius fit. Otherwise the planar chamfer event names an open common Scale:

    lambda = (W - 1000 max(tol_P, tol_Q)) / (a + b).

As in the prior parallel-line calculation, gamma_7 rounds that scale inward for
the subtraction, sum, division and subsequent scaled-setback comparison. The size
kind is chosen for the whole call: an equal pair inside a call containing unequal
chamfers must compute the open scale directly, rather than convert its closed
radius afterward. Radii and dimensionless factors are never compared as one size.

## Hand-derived regression values

The synthetic quadratic tip has control points (10, 0.6), (5, 0.6), (0, 5), all
weights one, above the segment from (0, 0) to (10, 0). At their terminal endpoints
the tangent directions are horizontal, the inward normals oppose, and the
separation is exactly W = 0.6. Explicitly the quadratic is
`C(t) = (10 - 10t, 0.6 + 4.4t^2)`, for `0 <= t <= 1`. Its vertical projection
stays on the bottom segment, so the separation is at least 0.6 everywhere and
attains 0.6 at t = 0. With first distance d on both top rims, a+b = 2d:

- At 80 degrees, the wall setback is d tan(80 degrees), while the top widths stay
  d. At either input d = 1 or d = 0.5, the named first distance is
  `(0.6 - 1000 tol) / 2`, 0.295 mm for tol = 0.00001 mm.
- At 45 degrees, the offsets are one common radius. The terminal contact is the
  closed radius 0.6 / 2 = 0.3 mm. The old station search reported 0.300523 mm because
  it excluded the endpoint event.
- A 30 x 2 x 10 box's opposite top edges chamfered 1.5 by 1.5 meet at radius 1,
  preserving the existing equal-offset test. With an additional bottom edge
  chamfered 1.5 by 3, the call needs a scale. The top pair leaves the margin at
  `(2 - 1000 tol) / 3`, not the closed factor 2/3.
- On that box with all four top rims selected, first distance 1.5 and angle 80
  degrees, the opposing long rims still have gap 2 and combined top width 3.
  Their closest set spans the length of the box; its midpoint is away from the
  selected endpoint corners. The open scale is again `(2 - 1000 tol) / 3`.
  At closed scale 2/3, the contact is (15, 1, 10). Its distances from the two
  incident end-edge lines x = 0 and x = 30 are both 15, strictly larger than
  their scaled widths (2/3) 1.5 = 1. Thus the contact survives the end mitres.

No stored golden or kernel-case expectation is changed to match an observed value.
The tests check the derived size, that the named size builds, and that the 0.2 mm
control still builds.

## Diagnostic propagation

The band event supplies its target edge, shared face and an interior point of that
face. The executive records this site after recording NoRoom, clearing stale
vertex/end-face data. Ops and SolidCraft preserve the limiting face separately
from the edge point. A distance-angle chamfer names the first distance at the same
angle; two explicit distances keep the factor that scales both.

## Validation boundary

The changed core test file passed all nine tests, including the curved-tip endpoint,
fixed-angle distance, mixed-size margin, selected-corner contact and existing
128-edge outline cases. Both workspace formatting commands passed, as did the diff
check and pending-translation language, ordering and placeholder checks.

The first application CLI build compiled, but its reproduction still failed to name
the fit before the exact B-spline translation-net recognition was added. That exposed
the design-record boundary described above. The final core test run compiled this
recognition; its ops record-round-trip regression and a fresh application CLI run
have not run. The subsequent CLI rebuild and ops area test job were cancelled while
queued under the owner's updated rule to stop after compilation and the core
regressions pass. Kernel/smlib area tests and clippy were not submitted. These wider
checks, and confirmation of the final application reproduction, remain for CI.
