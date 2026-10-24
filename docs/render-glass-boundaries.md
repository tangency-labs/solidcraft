# Closed-glass ray boundaries

Closed dielectrics use the uploaded triangle boundary for medium transitions.
A path that cannot classify its medium (an escape while inside glass, an exit
with no matching entry, or an entry while already inside) ends there, keeping
only the light it had already gathered; it is never converted into transparent
background. Each job counts these paths. A handful at the limits of f32
geometry (a few per billion on the captured 69-body tumbler scene) are logged
and tolerated; more than one in a million paths fails the job with the first
failure's pixel, bounce, sample and code. A camera inside glass, open meshes or
overlapping solids fail every affected path and therefore the job.

Surfaces may touch closed glass, as a tumbler resting on a table does. The
offset launch from such a surface can start inside the glass, so its first
crossing is an exit. A ray launched from an opaque or thin-sheet surface whose
first crossing exits a closed body starts in that body's medium, with Beer
absorption from the launch point; shadow connections do the same. Camera rays
and closed-glass continuations that meet an unmatched exit are still errors.

A scene-wide normal offset and positive minimum distance are unsuitable for a
closed volume: near a sharp rim they can jump across the next face or discard a
valid short exit. Closed-glass continuation instead starts at an affine
barycentric surface point, excludes its producing boundary, and considers every
strictly positive intersection. Primary rays and opaque/thin-sheet launch policy
remain separate. Finite emitters use the same minimum distance as the continued
ray. Beer attenuation measures between physical surface points.

Triangle candidates first undergo an exact coordinate-bound test: when the ray
is on or outside a stored coordinate bound and moves away, no positive hit is
possible. This uses comparisons, without an epsilon or rounded division. It
prevents cancellation in a projected intersection from turning a behind-origin
intersection into a false forward hit. This check is narrower than the padded
BVH broad phase.

The producing boundary is an exactly coplanar, edge-connected patch within one
body, computed from the positions actually uploaded as f32. Adaptive exact
predicates distinguish a plane from even an arbitrarily shallow bend; nearby
parallel faces and disconnected surfaces retain separate identities. Excluding
only this patch avoids a rounded surface point re-hitting its own plane at a
grazing angle. Patch identities add four bytes per triangle and their preparation
storage is included in admission. Nonmanifold or collapsed adjacency is not used
to infer a patch.

Projected edge determinants use a compensated difference of products. This
avoids losing exact shared-edge cancellation on tested GPU compilers. The
algorithm follows the arithmetic described by
[Matt Pharr's discussion of Kahan's difference of products](https://pharr.org/matt/blog/2019/11/03/difference-of-floats).
WGSL [floating-point evaluation rules](https://www.w3.org/TR/WGSL/#floating-point-evaluation)
permit implementation variation, including expansion of `fma`; backend tests
are required, and this is not a proof of exact intersection for all WGSL devices.

Hit depth also uses an affine base plus local vertex-edge increments. Summing
three weighted distances can round a distant sharp-rim entry and exit to the
same value and let traversal order choose the exit first. The captured coffee
ray has real crossings separated by 0.00010076 mm at about 929.52 mm distance;
its regression covers all axis permutations, coordinate reflections and both
triangle orders. This improves distance ordering without treating an unmatched
medium exit as transparent.

Candidates are ordered by that dominant-axis depth before it is scaled into a
ray distance. Scaling can round two different depths to one distance: a
captured 64-sample coffee camera ray crosses a bin rim at ray parameters
1124.0922606 and 1124.0923199, under one f32 ULP apart. The two depths still
differed by one ULP, but both scaled distances were equal. Traversal order then
picked the exit, and the path stopped at the medium gate. On an exact depth tie
the hit that changes the current medium state wins: the entry face from
outside, the exit face from inside. A ray through the shared rim edge itself
exercises that rule. Traversal order never decides between them.

The regression fixtures distinguish short real exits, a normal offset crossing a
sloped adjacent face, exact diagonal hits, and a captured impossible hit at a
polygon edge. Original editable CAD renders exercise hollow walls and repeated
reflection/refraction through the production command and job interfaces.

## Candidate anchor and local-offset continuation

A rounded world-space hit point can lie outside a sharp rib even when all three
barycentric weights are valid. The candidate continuation representation keeps
the dominant stored vertex as an anchor and keeps the two weighted edge
increments as a separate local offset. It is an anchor plus displacement, not an
error-free two-float expansion. All three barycentric weights are retained
directly; recovering a small weight as `1-u-v` can erase corner clearance.

Ordinary floating expressions do not establish an evaluation barrier under
WGSL. In the captured rib case, the compiler transformed a relative expression
equivalent to `(vertex-anchor)-offset` into subtraction from a rounded world
point. A float TwoSum/FMA residual formulation also lost its residual on that
backend. The candidate therefore evaluates nonzero-offset point deltas using
integer-defined binary32 addition and subtraction: align significands with a
sticky bit, normalize, and round to nearest with ties to even. The sequence is
`RTE(RTE(a-b)+RTE(a_offset-b_offset))`. No floating reassociation can change those
integer rounding boundaries. Zero-offset rays retain the existing floating
path. Integer bit results include subnormals; later floating consumers remain
subject to WGSL's allowed subnormal handling.

The candidate routes triangle coordinate bounds, relative vertices and hit depth
of triangles near the origin, analytic emitter intersections, and physical-point
Beer distances through this representation. Traversal itself does not: BVH boxes
are tested in ordinary arithmetic from the rounded origin `anchor+offset`, grown
by the scene broad-phase pad plus `2^-20` of the origin's largest coordinate
magnitude (several ULPs, covering the rounding of that origin). Every point the
true ray reaches at depth `t` lies in the grown box along the rounded ray at the
same `t`, so box rejection stays conservative. A triangle whose stored bounds
are separated from the rounded origin by more than `2^-8` of that magnitude on
some axis (about 4 mm at 1 m) uses the ordinary intersection from the rounded
origin: an origin error of a few ULPs cannot reorder a hit that far away.
It can still move the ray across an edge. In a concave rib, a ray leaving
through one face can re-enter the next face within micrometres of their shared
edge. The far face, tested from the rounded origin, then reported a miss
just outside that edge, while the nearby face across it, tested exactly from
the true origin, correctly missed too, so the re-entry was lost (captured at
256 spp: bounce 5, code 2). Far
candidates whose bound or edge decision lies within `2^-20` of the origin
magnitude, scaled by the projected edge length, therefore also take the exact
path. Only those and the nearby triangles, which include every sharp-rib exit,
use the integer-defined deltas. On the exploded coffee machine at 1080p
and 8 samples per pixel this cuts closed-glass time on an NVIDIA GB10 from
43.5 s to 5.7 s (opaque: 5.2 s); 3 of 2,073,600 pixels differ from the
all-exact result, by at most 6/255. For a finite emitter, it first subtracts the
previous physical point from the ray origin and then adds the ray segment `d*t`.
It does not add a short segment into a large local offset and subtract that
offset again. Area-light density uses the same resulting physical segment.

This is a specified sequence of rounded operations, not an exact four-term sum
or a universal watertightness proof. In particular, a large anchor difference
followed by cancellation against the offset may still lose small information;
edge projection, barycentric evaluation, and local-offset construction also
retain ordinary floating arithmetic. Acceptance requires the integer bit oracle,
captured rib/corner regressions, and production renders on each supported
backend. These candidate notes do not claim those checks have passed.

A closed-glass continuation within `1e-6` of tangent to its producing face
ends the path. It skims that face, and the rounding of its reconstructed start
point (a few `1e-8` mm on the captured 69-body tumbler scene) then decides
which side of the neighbouring faces it is on. The captured ray left a face at
a cosine of `4e-8` while inside a glass, and then entered the adjacent face of
the same glass from outside at a shared edge. Those directions carry about a
millionth of the hemisphere's projected solid angle; ending them removes
negligible energy, not a classified medium error.

The bounded opt-in glass shadow connection uses the same unshifted, zero-minimum continuation and exact producing-plane exclusion at closed interfaces. Thin-sheet shadow traversal retains its existing offset policy. A corner oracle checks a real exit shorter than the old global minimum while retaining both Fresnel interface factors and Beer attenuation.

Closed shadow continuation also retains the stored anchor and local offset. It
uses the same integer-defined point deltas for bounds, interfaces and Beer
segments. For a finite endpoint inside a medium, attenuation uses the current
ray-origin-to-entry difference plus the remaining ray segment; it does not form
a rounded world endpoint. Geometry crossings are tested up to the true endpoint,
without applying the separate analytic-emitter self-intersection allowance.
The scatter history used by area-light MIS carries its associated local offset.
