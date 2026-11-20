# Issue #17016: a concave partial rollover crosses non-tangent loft seams

Status: exact native and engine regressions pass, as do all 63 blend tests,
638 selected units and eight controls. Clippy passes for all changed crates.
The rebuilt CLI replays all 21 original commands and captures the completed
radius-1 round; before/after images are in reports/issue-17016/.
Exact unignored regressions remain in the existing engine and native blend test
files; no known-failing entry is added. Command results are tracked in
`reports/issue-17016/validation.md`.

## Exact input

The engine regression replays the report's 21 commands unchanged: six periodic
cyclic C2 fit-point sections, the default loft, a joined radius-6 cylinder, then
its radius-1 foot round. Before the round it has sixteen valid faces. The native
regression loads the exact pre-fillet SCBREP05 cache slice from the first twenty
commands saved by SolidCraft 0.44.1 (`e151d5ea6866`). SHA-256:
`b1eba0a699d687108f0015a7c12d6f394ab5fafe5a8e14f965f17892f3e6206a`.
The engine test independently reconstructs the input without the fixture.

## Immediate failure and rejected partial repair

`fillet_geom/p1.rs::find_fillet_brep_edge` compares whole-curve midpoints with
stitched edge midpoints. A rail [0,13.335922967680741] becomes pieces
[0,13.328593635367252] and [13.328593635367252,13.335922967680741]. Its long
piece midpoint is 0.003151274258630283 from the full curve midpoint, beyond the
existing 0.00004 edge tolerance, despite projecting onto that curve exactly.
The short piece projects within 7.715283210306083e-13. This explains the public
error, but fixing only ownership does not produce a valid body.

An exact construction-provenance experiment retained every split descendant,
inserted bounded immutable curves, refreshed owners after imprints, and trimmed
each rollover rail against its own support. All 172 affected unit tests and
33 existing native blend tests passed, but this exact input still returned
`NotSolid`: seven faces, twenty-nine edges, open. The experiment is removed.
All rail mates existed and all fourteen common edges were marked consistently;
no short-curve classification early exit fired. Missing mates or deletion marks
do not explain the open contact contours.

## Different support solves, not one lost event

Job 1794996860.614419-1df8aad5 measured the actual input seam near
(1.930002811263407, -6.646540304635047, 34.87185597423832).
Face 8's unit normal is
(-0.019401758572460567, -0.08631801817271943, 0.9960786974446497),
and face 10's opposing face-use normal is
(0.019398039536810702, 0.0883612412090529, -0.9958995969042882).
The unoriented tangent-plane angle is
180 - 179.8824828407804 = 0.11751715921960226 degrees.
The symmetric positive-Y seam has the same angle. These are not exact G1 seams.
C2 interpolation within each input section does not guarantee G1 loft patches:
section-specific chord parameter scales need not interpolate proportionally.

The fixture's face domains establish the material-side ordering: face 10 is the
inner top strip (v in [6,7], with the cylinder-foot hole); face 8 is the outer
negative-Y strip (v in [7,8]). They share seam 31 at v=7. Moving toward increasing
Y crosses from face 8 to face 10. Their outward top normals give dz/dy increasing
from 0.08631801817271943/0.9960786974446497 to
0.0883612412090529/0.9958995969042882, a positive slope jump. Since material lies
below these faces, this is a concave/reentrant crease. The normal magnitudes
alone, without the face-domain ordering, do not establish that sign.

The same stored spline definitions also show why whole-edge convexity is not a
certificate of this crossing. Seam 31's edge interval is [0,1]. The inner-minus-outer
dz/dy jump is positive near the actual rail contacts (u about 0.4812 to 0.5188),
but negative at u=1/11 (about -0.00788008) and u=4/11 (about -0.000844604).
Those are two of the existing ten whole-edge sample positions. The new rail
judgment uses the existing zero-angle material-sector sign criterion at the
classification's actual edge parameters, for both exits and returns. Its
`boundaries_into_material` result is false for an unclassified boundary, an
unjudged run, or a rail wholly off its support. The session consumes this result
instead of repeating the whole-edge sampled predicate. This changes neither the
intersection budget nor the material-sector sign criterion.

The first support solve assigns pseudo Vertex4 to
P=(1.9276412674800594,-6.6465405594731015,34.87180992155087).
Recovery then retraces the next patch and re-solves that same Vertex4 on its
other support, assigning
Q=(1.934614994257056,-6.646539751831397,34.87194573632894).
Euclidean distance |Q-P| = 0.006975049208889349 mm. Each patch's trimmed rail
agrees with its own event. Reusing P for both would break the second support's
ball-foot condition; reusing Q would break the first's. Welding those vertices,
picking a larger tolerance, or mapping one arbitrary stitched descendant cannot
establish a tangent valid round.

The existing Join criterion in `blend_corner.rs::join_within_xsect_tolerance_deg`
requires 2*r*tan(angle/2) below the model intersection budget. For r=1 and the
measured angle that scale is 0.00205106207501552 mm, already far above 0.00004.
This calculation diagnoses a geometric gap; it is no new acceptance tolerance,
expected output, or certificate of the full loft.

## Construction boundary

The tangent-rollover recovery re-solves shared RailXEdgeuse starts on the next
support. That requires a common section within the body's intersection budget;
the measured sharp seam does not have one. The fix uses the existing Join
section-gap criterion for fixed-offset solvers, capped by their original angular
tolerance, at the actual rail crossing. Recovered starts and crease events are
also tested at their actual edge parameters. No tolerance is increased.

A concave rail entering a concave wall is a crash into material, distinct from
hanging over a cliff. For fixed-offset solvers only, the fix retains the
original round on its supports' continuations and uses the existing
`trim_at_creases` construction: intersect the round with the adjoining faces,
keep its outside-material pieces, imprint the boundary and close the body.
It requires every off-face run to leave through a classified manifold concave
edge and all end sections to fit. The existing final closed/manifold gate is
retained. An unrelated convex overflow alongside the accepted crash still gets
`OverflowWithConcaveBlend`, in either solver order. Other hanging-cliff,
open-edge wrap, tangent-rollover and global-merge refusals stay in place. A wrapped
off-face run of a closed filleted edge has no physical end to overrun: its end
labels refer to the periodic seam. It may use the same crease trim only if both
the exit and return are classified concave boundaries. `return_across` records
the latter separately, preserving the corner Roll-over's exit-only contract.

Varying-radius and constant-distance solvers do not have a uniform offset bound
in their stored surfaces: setup overwrites those with the last evaluated law
radius. They retain their existing angular policy and are excluded from the new
concave-crash acceptance. The fix makes no varying-law certificate claim.

This uses one existing crease construction, with no retry, weld, changed spline
input, larger tolerance or success-by-refusal test. The final native regression demonstrates strict closure and CheckAll validity
before and after record save/reopen (job 1795055126.910187-90580c3d).

## Concave crash controls: independently derived sections

The first proposed positive control in the existing
`wr631_cliff_overflow_choice.rs` file had the counterclockwise profile
(0,0), (10,0), (10,10), (7,10), (7,5), (5,5), (5,10), (0,10),
swept twenty units along Z. Its radius-3 floor contact is x=4 along the entire
edge, outside the floor support x in [5,7]. There is no classified exit or
return. Job 1795021152.000711-08fe03c7 correctly refused it with ConcaveOverflow;
the initial positive expectation did not exercise the guarded construction.
It is retained as `a_concave_round_wholly_off_its_support_is_refused`.
The mixed version was likewise refused as ConcaveOverflow; it is replaced by
a finite-wall mixed control, rather than broadening acceptance of unknown runs.

The finite-wall control starts with the L profile
(0,0), (10,0), (10,10), (7,10), (7,5), (0,5), swept twenty units along Z.
Its area is 10*5 + 3*5 = 65. A wall over x in [0,5], y in [4,10],
z in [5,15] overlaps the floor by one unit and adds 5*5*10=250 volume.
The original volume is therefore 65*20+250=1550. The radius-3 floor contact
x=4 is on the floor for z in [0,5] and [15,20], and off between them,
crossing the wall's concave feet at z=5 and z=15. Both boundaries can be
classified, unlike the rejected uniform-notch control.

The radius-3 concave round at (7,5) has section
centre (4,8). The opposite wall x=5 cuts its lower-right quarter-circle
at y=8-sqrt(8). The kept arc lies over 5 <= x <= 7. Its added area is
integral_5^7 [3-sqrt(9-(x-4)^2)] dx. With u=x-4, the antiderivative of
the root is [u*sqrt(9-u^2) + 9*asin(u/3)]/2. Subtracting its values at
u=3 and u=1 gives added area
6 - 9*pi/4 + sqrt(2) + (9/2)*asin(1/3).
On the ten units outside the wall, the full section adds
A=9*(1-pi/4). On the ten units inside it, the clipped section adds
B=6-9*pi/4+sqrt(2)+(9/2)*asin(1/3). The expected rounded volume is
1550+10*A+10*B=1588.0631271376258. This value is derived from the input
planes and circle, independently of any build result. The mixed control adds
the separate one-unit-high ledge and checks OverflowWithConcaveBlend in both
solver orders; its concave rail now has the same classified boundaries.
No golden or kernel-case value is blessed.

## Intermediate exact-result failure before the UV correction

Job 1795021152.002107-01b86046 compiled the engine and ran both existing
rounded_loft_face regressions. The older end-face sketch test passed. The exact
six-section foot-round regression reached original-result validation, which
reported two 2D B-spline Test #13 failures: minimum curvature radii
1.3826634e-5 and 5.2671097e-6 against 0.0001000262 and 0.000099945577.
These are curvature radii, not projection gaps. This intermediate construction
was not valid; the native owner trace below identified the additional cause
without changing validity policy.

## The remaining UV cusp: a boundary edit, not a 3D crease cusp

The intermediate native run (1795030056.244931-2e001166) passed all eight overflow
controls and 62 of 63 native blend tests. The exact regression alone failed the
same two UV Test #13 checks as the engine. Both owners are on procedural blend
face 21: edge 54 / edgeuse 210, and edge 56, at opposite crease ends.
For edge 54 the first span is [0, 2.562890932455376e-5]. Its v coordinates
are 1, 1, 0.9999946776613239, 0.9999946776613239: a v change of
-5.322338675e-6 with both endpoint derivatives zero. The secant dv/dt is
-0.207669, so this is a transverse near-boundary span, not an isoline.
The failed parameter 2.56289093170882e-5 is the final knot of that span.
The corresponding 3D control-point increments remain approximately
(8.526e-6, -1.2e-7, 5.26e-7); there is no matching 3D cusp.

`surface_drop_lib/p3.rs` had applied `DropPt::fix_boundary_tangents` before
testing the span, then `fix_boundary_curves` before emitting it. Proximity to
a UV boundary does not prove the curve runs along that boundary. These moves
had no bound on a surface evaluated from its procedural definition.

The replacement for procedural definitions is one unchanged Hermite
construction: keep the traced UV points and derivatives, including oblique
boundary crossings. A cubic is inside the convex hull of its four control
points. Requiring all four finite and in the exact UV rectangle proves the
whole span stays in the surface domain. The check is made on every added span
and every flush, including intermediate flushes. It also proves the domain
clamp in the existing span accuracy test is inactive over this polynomial.
An unproved polygon is an error; no boundary snap or alternate fit is tried.
The separate pole dog-leg edit and endpoint clamping are disabled for this
policy because they would change the tested polynomial. Other surface classes
retain their existing construction. No tolerance or validity threshold changes.

The two focused tests in existing `surface_drop_lib/p2.rs` verify the exact
line v = 1 - 2^-20 u retains its nonzero transverse derivative through both
flushes, and that an out-of-domain or nonfinite control polygon is refused.
The final native job 1795055126.910187-90580c3d passes all 638 selected
unit tests, including these two, all eight overflow controls and all 63 blend
tests. The exact six-section regression checks the original result and the
record-reopened result with CheckAll. Engine job 1795055279.048636-fe7e02c6 also passes both rounded_loft_face
regressions (2 passed, 0 failed, exit 0), including the original 21-command
six-section loft and save/reopen. Full command results are recorded in
reports/issue-17016/validation.md.

## Review follow-ups

A rail that crosses a vertex shared with a tangent boundary edge. The face
classification names an edge only for a run end that lies on an edge; an end
classified at a vertex gave no seam event, so the crease refusal
(`CreaseAcrossTangentEdge`) did not see the crossing, though the same crossing
on the edge was refused. Each run now also records the vertex of an end
classified at a vertex, and each edge of the crease's face that meets that
vertex becomes a seam event at the vertex's own parameter on it (its interval
start where it starts there, else its end). The refusal then tests each of those
edges as before. The check is the existing tangent test; nothing new is
tolerated. The regression is
`a_crease_through_a_vertex_gives_a_seam_event_for_each_face_edge_there` in the
existing `trim_at_creases.rs` tests: a crease through a box's top corner gives
an event for each of the top's two edges there, and none for the vertical edge.

Only the face's own edges. The first version made every edge at the vertex an
event, whichever faces it bounds. That refused #2385's disc
(`issue_2385_crease_along_edge::a_round_just_past_its_wall_builds`, r 2.0001):
the round's crease on the bottom face runs along the bottom circle and its run
ends at the circle's vertex, where the side face's seam also ends. The seam
bounds the side face on both of its uses, so it is tangent (0 degrees); the
side is a face the round rolls on, and the crease was refused with
`CreaseAcrossTangentEdge`. But a seam between two uses of one face separates no
face from the crease's face: the crease leaves the bottom there across the
bottom circle, a 90 degree edge, which main judges and passes. The events at a
vertex are now `vertex::edges(b, v, Some(f))`, the edges of the crease's face
`f` there, and the disc builds as on main.
`a_crease_through_a_seam_vertex_crosses_only_the_face_edges_there` (the same
file) is the cylinder top's case: the seam is no event, the circle is, and the
crease is not refused.

A non-finite crossing evaluation. The concave test at a crossing was the
negation of the convex check, so a NaN dot product came out as concave, which
authorises crease trimming. The two dot products are now tested for finiteness
first and a non-finite one is a typed blend failure. The regression is
`a_non_finite_radial_sector_dot_is_refused_not_concave` in the existing
`fillet_solver/p5.rs` tests.

## Narrowed to the loft: what main's tests pin (maintainer review)

Three tests that pass on main failed on the PR's head:
`a_refused_trim_at_creases_leaves_a_body_that_saves_its_surfaces`
(`issue_2715_refused_fillet_keeps_geometry`),
`a_round_onto_a_tangent_round_fits_where_its_rail_stays_on_the_top` (`wr696_fit_sizes`) and
`tangent_rollover::a_convex_round_larger_than_the_round_beyond_is_not_rolled_over`.

Taking the section-gap tangency budget out left the trim tests failing the same way, so it
was not the cause; logging the trim's edges showed the cause, the crease trim's
`CreaseAcrossTangentEdge` test. On main it asks, for every edge of the face a crease
lies in, whether the face across it is a tangent round the blend rolls on (tested at 0.34
of the edge, against the solvers' tangency tolerance). The PR asked it only of the edges a
crease run ended on, where the crease crosses them. In the backed block (a 20 x 10 x 10
block, back top edge rounded 3, front top edge rounded 9) the crease runs along the top and
ends on the two side edges; the tangent edge to the back round is not an event, so nothing
asked about it and the round of 9 was built, where main refuses it.

Now the edges a crease crosses are tested where it crosses them, against the budget of
the round the face belongs to (a spline seam's tangency varies along it, which is what the
loft needs); every other edge of the face is tested as on main. The three tests and both
loft regressions pass. No tolerance was added: both budgets existed.

## Three goldens rows that move, and why

`blend.fillet_executive_p3`: `issue-880-half-arc-rim-0-setbacks-1-2`,
`issue-880-half-arc-rim-0-setbacks-1-1` and `source-file-nurb_box-edge0` still move after
the narrowing (`-1-2`: first term of `result_geometry.block` 1.1e-8 relative, third term
4.5e-9; `-1-1`: first mass term 1.1e-9; the box edge: 2.0e-9 and 4.4e-10).
Taking out the kept UV tangents (`BoundaryPolicy::Preserve`, `fix_boundary_tangents`
skipped) returns all three to their old values; taking out each of the other changes,
alone or together with it, does not move them. They are the intended change. In these
three the drop runs on a surface evaluated from its procedural definition (the only
surfaces `Preserve` is used on), where upstream's fix-up has no bound on what it moves.
Logging what the fix-up would have done on the same runs:

- `source-file-nurb_box-edge0`: the dropped UV curves lie on the domain's v = 0 side (v from
  1e-14 down to 8e-23) with a cross-boundary tangent component of at most 7e-13 against a
  unit tangent along u. The polygon fix-up would move nothing (0). In four spans a tangent of
  up to 7e-13 is zeroed on main and kept now. The input differs by rounding noise; the 2e-9 on the
  result is that noise carried through a mass integral taken to 1e-4. This last step is an
  inference from the size of the input change, not measured.
- `issue-880-half-arc-rim-0-setbacks-1-1` and `-1-2`: ten distinct spans end on the v = 1 side at
  3.6e-8, 2.0e-8, 8.1e-8, 3.3e-7, 1.3e-6 and 5.2e-6 short of it, heading to it with slopes
  of 5.3e-5 to 8.5e-4. Main zeroes those slopes (a flattening of a near-boundary oblique
  crossing, the same as in #17016's loft) and its polygon fix-up moves two curves' control
  polygons by up to 3.5e-6 and 2.2e-5 in UV. The exact jets are kept now, proved inside the
  domain instead of snapped to it.

The rows are re-blessed with those numbers; no other row of the unit changed (798 cases,
5 known failing, as before).
