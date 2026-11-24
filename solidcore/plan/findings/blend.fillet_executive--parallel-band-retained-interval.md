# Parallel chamfer bands: retain the complete minimum family

Addendum to `blend.fillet_executive--planar-chamfer-band-contact.md`.
Review comment 4623214586 on SolidCraft PR #19116 identified a valid gap: one
midpoint does not represent every retained point of a parallel minimum family.

## Construction

Native line maps remain rational expressions in their stored factors. Project the
second bounded line onto the first using exact expansion dot products, intersect
the two rational parameter intervals, then parameterize that overlap by t in
[0, 1]. The feet P(t), Q(t) have one positive homogeneous denominator and the same
affine displacement. Their difference is constant; no fitted line replaces either
native curve. A singleton overlap retains the fixed-cap endpoint certificate.

For widths a and b, contact C=(b P+a Q)/(a+b), gap G=Q-P, and a selected neighbor
through L with direction V and width r, clearance is exactly

    |(b P + a Q - (a+b) L) x V|^2 - |G|^2 r^2 |V|^2 > 0.

Clear the common homogeneous denominator. This is a convex quadratic in t.
Subtract its closed nonpositive interval from the retained domain. Include the
existing endpoint tolerance neighborhoods where a fixed cap is not certified;
these are also squared-affine quadratic exclusions. Unsupported selected corner
classes retain the existing certification limitation.

Quadratic endpoints are real algebraic numbers (u+v sqrt(d))/q, with q positive.
Compare them exactly by eliminating at most two radicals through sign-aware
squaring and reading error-free expansion signs. Negative discriminants exclude
nothing; a zero discriminant excludes its singleton root. Closed intervals that
touch leave no retained point at their shared endpoint. Linear and constant
constraints use the same interval subtraction.

Keep t=1/2 when it lies in a surviving component. Otherwise round the algebraic
component endpoints directly with exact comparisons and choose a representable
interior parameter. Arithmetic bisection is bounded by binary64's 1074 exponents
plus one adjacency step; there is no model tolerance, approximate root, station
search, or rebuild. A nonempty real component without a representable parameter
returns a typed arithmetic-resolution error. Rounded native evaluations still
pass the existing clearance certificate; a loss of exact clearance there is an
error, never a claim that the entire interval was covered. The existing native
shared-face point classifier remains the final domain check.

## Derived regression cases

All cases use a 30 x 2 x 100 box and setbacks 1.5 on its two long top rims. Their
closed contact scale is 2/(1.5+1.5)=2/3, everywhere along x in [0,30]. Unequal
selected sizes make the named bound the existing open common scale
`(2 - 1000 max_edge_tolerance)/3`, rounded inward by the existing gamma_7.

- A 24-unit x=0 setback covers x in [0,16] at the closed scale. The old midpoint
  x=15 fails, but (16,30) remains. Mirroring the selected end leaves (0,14).
- End setbacks 24 and 18 cover [0,16] and [18,30], leaving (16,18).
- End setbacks 24 and binary64(20.999999999) leave width
  `(2/3) (21 - binary64(20.999999999))` = `93825/140737488355328`,
  approximately 6.7e-10. The opposite
  end-rim event is `30/(24+binary64(20.999999999)) > 2/3`, so the long-rim
  contact remains limiting. This gap is found from boundaries, independent of
  any ordinary station spacing.

The three-rim cases and the unequal-wall four-rim cases check the named scale
and build at that scale. Equal-wall four-rim cases check the earlier polygon event
derived below. No existing expected value or golden is changed.

## Test correction from the first compiled run

Test change: the initial new assertion incorrectly expected Scale for every case.
With every top rim selected and equal offsets on each rim, the existing polygon
consumption check runs before the band check (p3.rs, refuse_polygons_taken_whole).
The rectangle's inradius is exactly 1, and note_open_fit subtracts 1000 times the
owning edge tolerance, so that event correctly names Radius `1 - 1000 tol`.
The original case inputs are retained. Those cases now check that derived radius
and build with common factor `named_radius/1.5`. The two three-rim cases still
require Scale `(2 - 1000 tol)/3`.

Additional four-rim cases keep the same top setbacks while doubling each wall
setback, with an explicit reference to the top face. They roll no constant ball,
so the polygon-radius event does not apply; the band event names the common Scale.
Their wall setbacks are at most 48, below the box height 100. These cases check
both surviving intervals and the narrow gap using the same derived scale.


## The closed event also decides overlap

The application regression uses a 30 x 0.6 x 10 box, first distance 1, and an
80-degree second side. On the common top face the setbacks sum to 2, so their
closed contact is at scale 0.6/2=0.3. At the requested scale the rails are at y=1
and y=-0.4, beyond the opposite original edges. Neither current rail lies in the
other band's strip; that sampled test therefore misses the overlap and reaches
crease trimming without the fit diagnostic.

An additional native-parallel-line certificate reuses the retained contact
construction, comparing the squared exact feet separation against the squared
sum of the requested widths. A smaller separation proves overlap independently
of where the current rails lie. This closed event decides refusal; the existing
open-margin fit only names the safe size. All previous rail checks remain. An
absent retained certificate does not claim clearance.

Before constructing corners, native line directions must be exactly parallel and
the exact infinite-line gap must be smaller than the requested total width:
`|(Pb-Pa) x Va|^2 < (wa+wb)^2 |Va|^2`. Coordinate differences remain expansions.
This necessary condition excludes unrelated and separated pairs; the additional
check never calls the general curve minimizer.
The existing kernel regression verifies the edge, limiting face and fixed-angle
first-distance diagnostic, and verifies common-scale wording for two distances.
