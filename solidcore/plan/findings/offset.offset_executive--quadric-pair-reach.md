# Local extension reach between curved quadrics

The reported draft in #16113 moves a cylinder beside spherical trims onto a cone.
The local operation bounded only edges with a plane among their new surfaces. Its
corner bound also needed the two kept quadrics to share an unchanged straight
edge crossed by the plane. A bore with spherical ends needs both missing constructions.

The shared construction in `offset/offset_executive/reach/quadric.rs` uses the
original edge point p(s) and tangent t(s), and displacement d as the unknown:

    Q_a(p(s) + d) = 0
    Q_b(p(s) + d) = 0
    t(s) . d = 0

At a corner, p is the vertex and the last equation is the third face's plane.
The displacement coordinates avoid subtracting world-coordinate root boxes to
bound a small local movement. Newton finds a centre only. Krawczyk's operator
proves a root for every section over each whole parameter piece, using outward
interval arithmetic and mean-value enclosures of the equations at fixed d.
Both cone radii must be nonnegative over the resulting box. Its displacement
norm bounds the reach. A determinant enclosure marks crossings only when the
section Jacobian is proved nonsingular. Uncertified pieces are subdivided within
the existing caps, or refused; they are never classified as separated geometry.
The unchanged-straight-edge construction is selected from topology where that
edge exists, including a cylinder split into coincident faces.

The cone polynomial and its exact gradient are the same polynomial used by
`surface::implicit::lies_on`: v.v - z² - (r + tz)², with z = axis.v. This avoids
assuming that the stored unit axis has exactly unit squared length. The existing
plane/quadric/net corner solve shares this value and gradient implementation.

References: R. Krawczyk (1969), *Newton-Algorithmen zur Bestimmung von Nullstellen
mit Fehlerschranken*; R. E. Moore (1977), *A test for existence of solutions to
nonlinear systems*; A. Neumaier (1990), *Interval Methods for Systems of Equations*,
§2.1. These are the interval inclusion and mean-value constructions documented
in `docs/definitions-and-certificates.md` and the earlier reach-beside-nets finding.
External papers could not be fetched in this environment.

Regression cases in the existing reach test module build a synthetic slab trimmed
to a sphere, with a cylindrical bore through its spherical ends. The full bore
exercises the curved-edge bound; a half-slab supplies plane/bore/sphere corners.
The expected meetings solve z² + (1 + tz)² = 9 in closed form. Tangent and
separated bores remain refused, and a wrong-nappe root is not accepted. A rotated
and translated corner checks the rounding enclosure away from the origin.

The first targeted run certified both constructions, preserved the tangent,
separated and wrong-nappe refusals, and passed the translated/rotated enclosure.
For the synthetic bore at slope 0.05 the exact largest displacement is
0.1511824264912026. The capped edge refinement returned the conservative bound
0.15122813199925766; sharpness to the body's tolerance is not required for a
certified upper bound. The corner agrees with the closed form to the existing
body-tolerance assertion. The edge test checks enclosure, a finite bound smaller
than twice the displacement, and every spherical edge marked as crossing.

Runtime tracing on the reported design confirms both missing kinds: cone–sphere
edges and plane–cone–sphere corners. Both the 1-degree and 5-degree drafts now
succeed as closed bodies, with volume increases consistent with an independent
first-order wall-displacement estimate. Final validation results and reporter
measurements are recorded in the pull request body; no attachment data is used
by the regression tests.

Validation: all 44 targeted reach/net tests and kernel clippy on all targets with
warnings denied pass. Restoring the previous unsupported dispatch makes both
synthetic bound regressions fail with `ReachUncertified`, as required. Formatting
passes. The offset executive p1 goldens pass all 1060 cases unchanged (95 known
failures). The p2 and local-operation golden comparisons are blocked by missing
pinned upstream test files; no golden is blessed. No complete checkout or pinned
reference objects could be recovered from available source/cache locations.
