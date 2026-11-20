# Healer face-property checksums include seam decisions

SolidCraft #18804 review follow-up. CI reportedly changed 17 `heal.face_props`
rows in `values.v.block[]`, with relative checksum differences around `2e-9`
and no row-level error or topology change. The exact old/new triples and the
CI commit are not yet available. No golden is changed on that evidence alone.

## The seam check is reachable

`crates/goldens/src/units/heal_face_props.rs::rust_props` constructs the face,
adds its down-side face use, and executes the script on the same mutable B-rep.
Ops 0 and 6 call the healer's `set_props_with_query_gaps`,
`set_props_stage2_mut`, or `set_props_stage3_mut`.

`crates/solidcore/src/heal/face_props.rs::set_props_stage2_with` calls its seam
query whenever `closed_u` or `closed_v` is true. Its mutable query helper
`face_has_seam_problem_mut` calls
`topology::face::p4::has_seam_problem_in_brep_mut`. That reaches
`has_seam_problem_mut`, `classify_seam`, and the new `seam_is_crossed` added by
commit `2afa709c094b462c1af434a61579e3f266d3723c`. The latter calls
`Curve::proved_disjoint` for candidate edges.

Thus this golden unit does exercise the changed seam decision. The new proof
reads geometry without modifying the classification or the B-rep. A direct
expected effect is a changed crossing flag and direction, not small numerical
drift in geometry. Stage 3 and later script ops can also observe those flags.

## A checksum difference is not a floating geometry difference

The unit's `snapshot` stores integer flags as floats alongside coordinates.
It includes unset integer counters with value `123456789`. These dominate many
stored sums. `golden.rs::floats` compresses every 256 entries of a long array
into `[sum, sum_abs, sum((j+1)*value[j])]`, with the weight restarting at 1 in
each block. The checker compares those triples, not individual entries.

An ordinary script op emits one status followed by an 82-entry snapshot.
The snapshot's `bad_crossed_seam` and `bad_crossed_seam_dir` have zero-based
indices 24 and 25, so their global indices for op j are `83*j+25` and
`83*j+26`. Ops 8 and 9 emit a different 83-entry loop snapshot; the same
positions there do not represent face seam flags.

A removed U crossing changes the pair from `[1,0]` to `[0,4]`: `Tri::True`
becomes `False`, and `SurfParamType::U` becomes `Neither`. A removed V crossing
changes `[1,1]` to `[0,4]`; removing Both changes `[1,3]` to `[0,4]`.
Their exact sum changes are therefore +3, +2, and 0. The weighted checksum
still distinguishes all three changes. A relative change near `2e-9` can
therefore be a deliberate integer seam decision against a billion-scale sum.
Row-level status, topology, and unchanged vector length do not rule this out.

For example, `gen0009` has five face snapshots followed by an op-8 loop
snapshot. If its old U crossing is the only changed decision, the five pair
changes imply these exact checksum deltas:

| Block | Sum | Absolute sum | Weighted sum |
|---|---:|---:|---:|
| 0 | +9 | +9 | +993 |
| 1 | +6 | +6 | +371 |

The committed sums are about `3.333336487e9` and `2.222254349e9`; those integer
changes are about `2.7e-9` relative. This is a prediction to check against the
actual CI triples, not a measured result or permission to bless.

## The six named generated cases

The inputs below were reconstructed from the repository's FNV-1a seed and
xoshiro256** generator, including the script guard. Each accepts the first
draw. The reconstructed op counts times 83 match the stored vector lengths.
Every surface has a full 360-degree sweep and polygon loops, so each can
reach the new native seam proof. The script op indices in the last column
are zero-based face snapshots that retain the stage-2 seam decision; the
other ops are loop snapshots.

| Case | Surface | Polygon loops | Ops | Face snapshots |
|---|---|---:|---:|---|
| gen0009 | Torus R=3.207512703810031, r=1.6037563519050155 | 2 | 6 | 0,1,2,3,4 |
| gen0017 | Cylinder r=0.9982728906113065, swapped | 3 | 7 | 3,5 |
| gen0169 | Cylinder r=1.7381881127468928 | 1 | 5 | 0,1,2 |
| gen0182 | Sphere r=1.691180808022763, profile -90..40 degrees, swapped | 2 | 11 | 0,1,3,4,5,6,7,8,9,10 |
| gen0205 | Cylinder r=2.8833582789918113, swapped | 1 | 4 | 0,1,2 |
| gen0348 | Sphere r=1.5687198829205549, profile -90..90 degrees | 2 | 4 | 0,1,2 |

The polygon edges are constructed as bounded `Line::from_points` segments.
For a stored line `P + t*scale*V`, its only possible intersection with the
meridian plane y=0 is at `t=-P.y/(scale*V.y)`. At that parameter its x is
`P.x-P.y*V.x/V.y`. Treating the stored binary64 data as exact rationals
makes these tests exact; no geometric tolerance is involved. Segments with
no parameter in [0,1] cannot meet the meridian. The following displayed
coordinates are shortened diagnostics of those rational computations.

- `gen0009`: all four segment crossings with y=0 have x at most
  1.319416711462102. The U seam's whole native profile has
  x at least R-r=1.6037563519050155. The V seam lies in z=0, while every
  polygon edge has z=0.5397982058458501. Neither stored 3D curve meets its seam.
- `gen0017`: the only y=0 segment crossings have x=-1.0345980227064502
  and 1.306541309214644. Neither is the cylinder meridian's constant
  x=0.9982728906113065.
- `gen0169`: the corresponding x values are -0.15726383028585247 and
  1.8950076536851987, neither equal to the constant x=1.7381881127468928.
- `gen0205`: the x values are -0.7590915554331509 and
  1.0697409565839704, neither equal to the constant x=2.8833582789918113.
- `gen0182`: the positive-X profile cannot meet the negative x crossing
  -1.3943460211515075. The other crossing, x=2.230977463899257, lies beyond
  the profile's maximum x=r=1.691180808022763.
- `gen0348`: negative x crossings cannot meet the positive-X profile.
  The positive crossing x=2.352507220493019 exceeds r=1.5687198829205549.
  The remaining positive crossing is x=0.1745867896937177 at
  z=-0.01579921611239321. Both coordinates lie inside the central square
  `0 < x < r/2`, `-r/2 < z < r/2`, which the meridian excludes: splitting
  each rational quarter arc once gives pieces whose entire native control
  hull has either `x >= r/2` or `|z| >= r/2`.

These checks predict what the original stored-curve exclusion would remove;
they do not justify False/Neither for the boundaries on their surfaces. The
stored curve is an approximation of an exact boundary, and projecting an
off-surface polygon can cross a seam its 3D curve misses. These changes must
not be blessed on native-curve nonintersection alone.

## Cause: the stored curve is not the boundary authority

`docs/validity-and-tolerances.md` section 5.2 defines the exact edge by the
incident surfaces; its stored C and pcurves are representations. Section 6
requires explicit seam topology without a periodic trim jump. The seam
classification deliberately uses `SourceReach::Boundary`; the healer's
`split_edges_at_seams` skips faces whose crossing flag is False.

For a cylinder of radius 1 with its seam at x=1,y=0, a rectangular stored
boundary at x=1+delta spanning negative and positive y is disjoint from the
3D seam. Its projection onto the cylinder nevertheless crosses longitude
zero. Thus the original `seam_is_crossed` filter could incorrectly clear
exactly the flags this golden unit observes.

No replacement exclusion is committed in this follow-up. A nominal incident
surface is insufficient authority for arbitrary imported/healed boundaries
(section 8), and a whole raw control hull can be too coarse even for the
reported rotated cylinder. The original filter needs a certificate for the
actual boundary on the current surface, or intersection-definition provenance,
before its exclusion can be used safely. This is a correctness blocker for
blessing the reported changes, not a request to loosen the golden tolerance.

## Pending validation

Obtain the complete 17-case list, old/new checksum triples, and CI commit.
Check every observed delta against individual snapshot fields and derive any
additional changed status or loop property before considering a golden
update. The six geometric nonintersection checks above do not by themselves
identify which of the compressed fields CI changed. No expected file has
been edited or blessed in this investigation.

## Checksum predictions before the local unit run

Applying the snapshot indices above to the six named scripts predicts the
following block deltas if the removed crossing flag/direction is their only
changed output. These are integer calculations from the op indices, not
measurements or a golden update. For each snapshot j, subtract weight
`((83j+25) mod 256)+1` and add `(4-old_direction)` times weight
`((83j+26) mod 256)+1`, accumulating in each corresponding block.

| Case | Block | Sum | Absolute sum | Weighted sum |
|---|---:|---:|---:|---:|
| gen0009 | 0 | +9 | +9 | +993 |
| gen0009 | 1 | +6 | +6 | +371 |
| gen0017 | 1 | +4 | +4 | +414 |
| gen0169 | 0 | +9 | +9 | +993 |
| gen0182 | 0 | +4 | +4 | +276 |
| gen0182 | 1 | +6 | +6 | +621 |
| gen0182 | 2 | +6 | +6 | +579 |
| gen0182 | 3 | +4 | +4 | +192 |
| gen0205 | 0 | +6 | +6 | +663 |
| gen0348 | 0 | +9 | +9 | +993 |

## Local validation was blocked by the queue limit

The requested unit check was submitted through buildq with the lane TMPDIR:

```
buildq run solidcraft . -- cargo --config 'env.TMPDIR.value="/home/ray/builds/lane-18804/tmp"' --config env.TMPDIR.force=true run --manifest-path /home/ray/builds/worktrees/lane-18804/solidcore/Cargo.toml --profile goldens -p goldens -- --unit heal.face_props --goldens
```

It remained queued for 30 minutes and was canceled before execution (exit
130); no cases ran. Temporary diagnostic changes to the golden runner were
removed completely. GitHub log access also failed from the sandbox, so the
complete 17-row list and old/new triples remain unavailable. The six named
cases' geometric and checksum derivations above are predictions, not a
claim that all 17 changed rows have been verified. No golden was blessed.

## Boundary authority: a disjoint stored curve is insufficient

`docs/validity-and-tolerances.md` section 5.2 says that an edge between faces
represents the intersection of their defining surfaces; its stored 3D curve
and its two pcurves are representations. Section 6 requires seam topology
where the boundary on the surface crosses the periodic seam, and forbids a
periodic jump in the trim. `docs/booleans.md`, "Seams and poles", consequently
keeps `HasSeamProblem` on `SourceReach::Boundary`: it reads both tolerance
bands, including boundary representation gaps. The original stored-curve
separation filter discarded that distinction.

The diagnostic regression `an_offset_boundary_still_crosses_its_cylinders_seam`
was constructed in the existing `topology/face/p4.rs` test module. Its one face is the
radius-one cylinder about z with natural height range -1 to 2. Its boundary
is represented by four straight edges through the rectangle

```
x = 1 + 2^-20, y = +/-2^-9, z = 0 or 1.
```

All coordinates are exact dyadic inputs. Every stored edge has x strictly
greater than the seam's x=1, so all are proved disjoint from the whole 3D
meridian. Their projection onto the cylinder has longitude
`atan2(y, 1+2^-20)` and the same z. Each horizontal boundary therefore crosses
longitude zero at its interior point y=0, at z=0 or z=1. These are interior
points of the cylinder's natural seam, not its endpoints. No seam vertex or
edge has been marked. The expected crossing flag is True in direction U.

This is an ordinary boundary representation within the existing tolerance.
Its radial gap is at most `2^-20 + (2^-9)^2 / (2*(1+2^-20))`, by
`sqrt(X^2+y^2)-X <= y^2/(2X)` for positive X. That bound is less than `3*2^-20`,
which is less than half the default kernel tolerance `1e-5`; neither the test
nor the construction raises a tolerance. The test separately asserts the
stored-curve disjointness, the raw classification's crossing candidate, and
the final face-property crossing flag. Clearing only the final flag on the
basis of native 3D separation fails this regression.

A sound exclusion needs the actual boundary's defining geometry. A proposed
replacement based only on another incident nominal support was withdrawn:
section 8 permits imported boundaries whose nominal supports do not meet
exactly. Adding the plane x=1+delta to this cylinder rectangle gives that
same counterexample with two incident faces. Mere incidence does not prove
that the represented boundary is the exact intersection of those supports.
An intersection-definition certificate or a boundary-on-surface certificate
is needed; neither is established by the existing filter.

There is also an independent completeness limitation in that proposal.
The notch cylinder is rotated by 0.6131 radians. Its full rational control
hull can extend below the true minimum Y by the rotated square's excess
`r*(sin(0.6131)+cos(0.6131)-1)`, approximately 0.027 at the smaller radius.
Thus refining only the seam against that fixed support box cannot certify
the reported positive gap. No tolerance change, tuned work cap or analytic
special-case retry was substituted for the missing construction.

## Disposition

The two requested review changes (the retained-meridian assertions and the
subnormal hull bound) are independent of this authority problem. The original
filter remains unchanged in this follow-up. Its 17 reported checksum moves
are not blessed: native-curve nonintersection alone does not justify them.
A correct exclusion for both kernel-built and imported boundary definitions
requires the additional certificate above. The diagnostic rectangle recipe
and expected True/U flag are recorded here so that work can proceed with a
maintainer-approved construction instead of forcing the attempted exclusion.


## Boundary exclusion now reads the face's own trim curves

`seam_is_crossed` no longer compares the stored 3D edge curve with the 3D seam.
For each use of the candidate edge on this face it takes that edge use's own UV
trim curve (never the mate's, which lies on the other face's surface) and
requires `Curve::proved_disjoint` to separate it from both seam lines of the
periodic UV domain (`u = min` and `u = max`, or the `v` pair). An edge use
without a UV trim curve, or a proof that cannot be completed, grants no relief
and the crossing stays. The offset-rectangle regression
`an_offset_boundary_still_crosses_its_cylinders_seam` has no UV trim curves, so
it keeps its crossing flag (True, U).

## Golden rows

`goldens --unit heal.face_props --goldens` with the boundary-based exclusion:
1873 cases, changed=0 added=0 removed=0 broken=0. No row differs from the
committed golden, so nothing is derived or blessed; the earlier CI differences
came from the stored-curve exclusion and are gone with it.

## Executed counterexample

The diagnostic ran through buildq against the existing seam filter, together
with the proximity module tests. Compilation succeeded. All nine proximity
tests passed, including the new subnormal regression. The offset-boundary
diagnostic failed exactly at `props.bad_crossed_seam.is_true()` after its
stored-curve disjointness and raw-classification crossing assertions passed.
Result: exit 101, 9 passed, 1 failed, 5277 filtered out. This confirms that the
filter clears a real boundary-on-surface crossing; the report is not inferred
only from the compressed golden checksums.

```
buildq run solidcraft . -- cargo --config 'env.TMPDIR.value="/home/ray/builds/lane-18804/tmp"' --config env.TMPDIR.force=true test --manifest-path /home/ray/builds/worktrees/lane-18804/solidcore/Cargo.toml -p solidcore --lib -- curve::proximity::tests topology::face::p4::tests::an_offset_boundary_still_crosses_its_cylinders_seam --nocapture
```

The temporary diagnostic was then removed from the final patch, so this
follow-up does not add a knowingly failing test without its required feature.
Its complete source remains in the lane's temporary validation artifacts; the
geometry and expected result above specify the regression to restore in this
same existing test module when the boundary certificate is implemented.

## Represented seam edges and shared crossing verdicts (75723f017 follow-up)

The own-boundary change in 75723f017 correctly keeps the offset-cylinder
counterexample crossed, but the unchanged notch regression again failed at
major=1.8 and d=1e-5: `SmFace::Test #29 Face crosses Seam`. The targeted
baseline ran through buildq and exited 101 (0 passed, 1 failed).

A trim disjoint from both periodic sides is not the only edge that cannot
cross them. The represented seam itself has two uses on the same face and
lies along a periodic side. Such an edge bounds no material across that face
(`edge::face_on_both_sides`). The exclusion now requires either every use on
this face to be disjoint, or that topological seam condition together with
every use's whole trim lying on a periodic side. A mixture of coincident and
disjoint uses is not accepted as a represented seam. A coincident edge shared
by different faces is still a material boundary, not relieved by coincidence.

The coincidence proof is a coordinate identity over the complete UV curve.
For a line, its stored canonical point has the fixed coordinate and its
stored direction has zero in that coordinate. For a positive-weight native
B-spline, every homogeneous control satisfies `X=cW` (or `Y=cW`). Existing
error-free expansion products and subtraction prove those equalities, rather
than comparing rounded products. Finite ordered knots, a valid nonempty net,
finite coordinates and positive weights are required. No geometric tolerance
is used. Trim lookup may resolve a mate only when its holder is on this same
face; a trim on another face supplies no evidence about this boundary.

The finite seam segments also require a coverage certificate. If an endpoint
is enclosed by coordinate p and error e, then `up(p_start+e_start)` and
`down(p_end-e_end)` give an inward interval covered by the entire line.
Intersect those intervals for both periodic sides. Each trim's outward hull
interval in the varying coordinate must lie inside that covered span before
finite-segment separation can exclude an infinite-side crossing. This rules
out a trim that crosses the seam outside the tested segments. These are
rounding bounds from the existing predicates, not new tolerance constants.

The missing-U and missing-V branches now receive touch significance from the
same cached per-edge verdict as the main crossing check. Each raw touch's
`pt_classify_index` names the first interval's start for index 0, otherwise
the preceding interval's end. It is read before endpoint review, which can
clear that classification while leaving the raw touch list intact. A safe
edge elsewhere in the connected loop cannot excuse a crossing edge. Vertex,
unknown and unresolved touches remain significant. Raw and processed touch
lists retain their original records.

The added unit checks derive their geometry directly: constant-coordinate
lines and rational controls lie along the seam; `x(t)=(2t-1)^2` only touches
it, and `x(t)=2t-1` crosses it. A coefficient equal to `fl(0.1*0.3)` does not
satisfy the exact identity with fixed coordinate 0.1 and weight 0.3. The
coverage case crosses u=0 at v=10 beyond seam segments spanning v=-1..2.
The raw-touch check uses two different edge witnesses connected to one loop,
and verifies that only the certified edge receives relief.

The native 3D separation predicate receives an exact embedding of the UV
trim as (u,v,0). A 2D spline stores `NOZ` in its homogeneous z coordinate,
which the 3D hull predicate cannot bound. `clone_exact` and
`convert_to_3d` preserve X,Y,weights and knots and replace only that z with
zero. A line's exact copy uses the existing 2D/3D conversions to zero its
stored point/vector z while keeping its native X,Y parameter map. Thus a
stored z never supplies false UV separation, and a legitimate 2D spline is
not rejected merely because its z is a dimension sentinel. The added check
places a whole native 2D spline strictly between the seam lines.

A native net also passes its existing `counts_are_valid` predicate before a
coordinate identity is used. In particular mixed sentinel/non-sentinel
weights are not a rational net and cannot receive that identity certificate.
The shared certificate is constructed lazily on the first edge witness,
leaving faces with only vertex touches on their existing query path.

A line also requires finite analytic-domain endpoints and length. Its stored
NURBS interval can remain finite after `set_analytic_domain` changes the STEP
domain; a nonfinite STEP domain makes evaluation undefined even if the raw
point and direction have a constant coordinate. The identity test rejects
that input rather than using the raw coordinate as a boundary certificate.

## Executed obstruction: the notch trim really crosses the seam

The represented-seam distinction and shared raw-touch verdict do not resolve
this particular notch. The unchanged boolean regression still fails at
major=1.8, d=1e-5, face test 29. The rejected witness is edge 3, a material
boundary (`face_on_both_sides` is false), not the retained meridian.

The diagnostic also measured its actual edge interval, [0,
0.6383563509772736]. Thus using the edge's interval instead of the whole UV
curve does not remove the following offending span. Its degree-3,
nonrational UV curve has the cubic Bezier span [0.5489169576060997,
0.6033954655313376], with U controls (indices 75 through 78):

```
A =  1.6790073919589227e-5
B =  6.785747257104085e-7
C = -5.668788196567444e-9
D =  0
```

These decimals name the stored binary64 coefficients, treated as exact
binary rationals. In the span's local parameter t,

```
q(t) = A(1-t)^3 + 3B(1-t)^2 t + 3C(1-t)t^2 + Dt^3.
q(1 - 2^-20)
  = -22602712198109640343817633659
    / 1393796574908163946345982392040522594123776
  < 0.
```

The value is approximately -1.6216650697106868e-14; its sign above is an
integer/rational calculation, not a tolerance comparison or a rounded test
output. Since q(0)=A>0, continuity proves a U=0 crossing strictly inside
this used span. There is another crossing at its final zero knot: its
incoming derivative is -3C>0 and the next cubic's first handle is
2.9866023517324607e-9>0. The preceding negative values continue into positive
values after the join. This is neither whole-curve coincidence nor a loose
control-hull overlap. The lane artifact `notch-uv-excursion-18804.json`
records the exact arithmetic and the diagnostic job is
`1795100598.325753-f52658da`. The interval-aware rerun,
`1795103072.228231-4eab9aa1`, confirms that the span is used and still fails
(0 passed, 1 failed, 69 filtered out; exit 101).

The UV Hermite construction in `intersect/surface_intersector/p2.rs`
forms its penultimate handle as `pt4 - drv2/3`. A zero endpoint with this
positive incoming U derivative necessarily gives the negative handle.
The existing certified shape correction following it supports spheres;
it does not supply a domain-preserving torus fit. The imprint copies the
intersection UV representation into the new edge
(`topology/proto_topology/p2.rs`, the `xsect_uv_curve(1)` copy).
`docs/booleans.md`, “Where exact edges still get fitted trims”, explicitly
records that traced SSI UV fits are checked at samples, not certified.
`docs/ssi.md` invariant 6 requires trims to stay in domain and to split at
real seam crossings.

Clearing this material edge as a represented seam would hide an actual
crossing of the face's used trim. Clamping only its negative handle would
still leave an interior seam touch at the zero knot. A correct repair needs
a certified domain-preserving SSI trim construction that resolves that joint,
or an intersection-definition certificate establishing a different exact
boundary authority. Neither can be replaced by nominal incident supports:
the two-face imported-boundary counterexample above remains applicable.
No such relief, new tolerance, retry or underived expectation is introduced.
The notch part of the brief remains blocked on that construction.

## Validation of the partial review changes

The separator uses the edge's actual parameter interval. Its added interval
case uses u(t)=-1/2+t: the whole curve crosses U=0, while its used interval
[3/4,1] has U in [1/4,1/2] and is separated. The same cached per-edge verdict
feeds the main crossing test and missing-U/V touch flags; represented seams
require two same-face uses and whole-curve coincidence.

All commands ran from the worktree root through `buildq run solidcraft .`,
with `TMPDIR=/home/ray/builds/lane-18804/tmp` and the same forced temporary
directory in the test/tool commands' Cargo environment:

```
cargo --config 'env.TMPDIR.value="/home/ray/builds/lane-18804/tmp"' --config env.TMPDIR.force=true test --manifest-path /home/ray/builds/worktrees/lane-18804/solidcore/Cargo.toml --profile goldens -p solidcore --lib -- seam --nocapture
```

Exit 0: 118 passed, 0 failed, 5318 filtered out. This includes the unchanged
`an_offset_boundary_still_crosses_its_cylinders_seam` and the three new cases
`seam_trim_identity_uses_the_whole_curve`,
`seam_segments_must_cover_the_whole_trim` and
`raw_seam_touches_use_their_original_edge_verdicts`. The first attempt stopped
at a new helper's overly restrictive mutable trait bound; the helper only
reads topology and now requires `FaceBrep`. Its two qualification warnings
were also removed before the passing run.

```
cargo --config 'env.TMPDIR.value="/home/ray/builds/lane-18804/tmp"' --config env.TMPDIR.force=true run --manifest-path /home/ray/builds/worktrees/lane-18804/solidcore/Cargo.toml --profile goldens -p goldens -- --unit heal.face_props --goldens
```

Exit 0: 1873 cases, changed=0 added=0 removed=0 broken=0 known_failing=0.
No committed golden row differs, so none is blessed. This run includes the
interval-aware separator and shared touch verdict. The existing boolean
file, its helper and its assertions remain unchanged.

The unchanged notch regression ran with:

```
cargo --config 'env.TMPDIR.value="/home/ray/builds/lane-18804/tmp"' --config env.TMPDIR.force=true test --manifest-path /home/ray/builds/worktrees/lane-18804/solidcore/Cargo.toml --profile goldens -p solidcore-ops --test boolean a_notch_beside_a_half_torus_seam_keeps_its_boundary_on_the_seam -- --exact --nocapture
```

Each run exited 101 with 0 passed, 1 failed and 69 filtered out, at the same
first notch (major=1.8, d=1e-5): baseline job
`1795098772.380967-571f6921`, represented-seam attempt
`1795099738.976947-44f100ec`, XY diagnostic
`1795100598.325753-f52658da`, and active-interval diagnostic
`1795103072.228231-4eab9aa1`. The last run establishes the used interval
quoted above. The temporary diagnostic output has been removed.

Both final formatting checks exit 0, as does `git diff --check`:

```
cargo fmt --all --check
cargo fmt --manifest-path solidcore/Cargo.toml --all --check
```

The first all-target lint attempt exited 101 for `bool_to_int_with_if` and
`redundant_closure_for_method_calls`. They were corrected to
`usize::from(direction != SurfParamType::U)` and
`PointClassification::edge_object`, respectively. These are equivalent
expressions; they change neither certificate nor test outcome. No lint
allow attributes were added, and both formatting checks were repeated after
these edits.

The corrected all-target lint run also exits 0 (job
`1795104724.151288-f1555bd1`, 106 seconds executing, queue wait below 30
minutes):

```
cargo --config 'env.TMPDIR.value="/home/ray/builds/lane-18804/tmp"' --config env.TMPDIR.force=true clippy --manifest-path /home/ray/builds/worktrees/lane-18804/solidcore/Cargo.toml -p solidcore --all-targets -- -D warnings
```

No checks were skipped for the queue limit. Only the wider suite remains for
CI; the unchanged notch regression remains an explicitly demonstrated
construction blocker, not a passing result. The changes are unstaged and
`COMMIT_MSG.txt` describes the partial review fix and this blocker.
