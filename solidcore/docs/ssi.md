<!-- SPDX-License-Identifier: Apache-2.0 -->

# Surface/surface intersection

This page describes how solidcore intersects two surfaces: the theory the code rests on, the
invariants a change must keep, where each step lives, what is measured and what is open. It is
written for a reader who knows surface intersection and is new to this code, and for anyone stuck on
an intersection bug. Numbers in brackets are references (end of page). Every statement about the code
names the file it was checked in; paths are relative to `crates/`.

There are two implementations today, and [layers.md](layers.md) §2 says one must go:
- **the port**, `solidcore/src/intersect` and the analytic special cases in `solidcore/src/surface`:
  SMLib's intersector carried over unit by unit, with the fork's fixes behind `Kernel::fixes` flags.
  This is what the boolean, the blend and every other caller run today;
- **`ssi`**, `ssi/` with its bridge `solidcore-ssi/`: a method of its own, written without reading
  SMLib, measured against the port on the SSI corpus. Nothing in `crates/solidcore` calls it yet.

Which copy survives is decided from the SSI corpus and the rig, and recorded on the job's issue.

The theory starts from a measurement: [#17](https://github.com/tangency-labs/solidcore/issues/17)
scored the upstream intersector on 119 pairs with exact answers and found that every miss sits where
the two normals are parallel. §3 is that measurement; the rest of the page is how the code handles,
or still fails, those places.

Contents:
1. [Purpose and the one rule](#1-purpose-and-the-one-rule)
2. [Scope](#2-scope)
3. [The corpus and the measured gaps](#3-the-corpus-and-the-measured-gaps)
4. [The pipeline](#4-the-pipeline)
5. [The exact classes](#5-the-exact-classes)
6. [Certification and tolerances](#6-certification-and-tolerances)
7. [Failure modes](#7-failure-modes)
8. [Invariants](#8-invariants)
9. [Status](#9-status)
10. [If you're stuck](#10-if-youre-stuck)
11. [References](#11-references)

---

## 1. Purpose and the one rule

**An intersection curve is exact where its class allows, and a certified exact B-spline otherwise.**
A plane and a quadric, and the quadric pairs with a closed form (§5), meet in a line, a circle or an
ellipse, and that is what is stored. Every other curve is a B-spline whose distance from the true
intersection is bounded by a proof, never by a sample. Nothing is a fit through sampled points that is
then hoped to be close enough. The pcurves (the curve's UV image on each surface) obey the same rule:
an exact rational curve in the surface's parameters where one exists, a certified B-spline otherwise,
built together with the 3D curve rather than projected onto the surface afterwards.

The rule is the target. §6 and §9 say where the code meets it today and where it does not: the exact
classes do; the general curves of both implementations are cubic Hermite interpolants through Newton
points, checked at interior samples, not yet certified.

The other rules follow from that one:
- **Points come from roots.** Every point on a curve is a Newton root of the intersection system, or
  a bracketed root of a field. No point is chosen by thresholding distances at sample points
  (`ssi/README.md`).
- **Regions are dropped only by proof.** A piece pair leaves the search when boxes, slabs or a
  field's coefficients prove it apart, never because a sample missed.
- **A contact that cannot be proved is refused by name**, never approximated (`ssi::Error::Tangent`,
  code `BOOLEAN_TANGENT`).
- **The topology is part of the answer.** A curve ends where two branches cross; a touch point is a
  point; a coincident region is its boundary. A complete point set with the wrong topology is a wrong
  answer (the corpus grades it so, §9).

## 2. Scope

**In.**
- Surface/surface intersection: `Surface::global_surface_intersect` and its analytic special cases;
  the general intersector (`intersect/surface_intersector`, `intersect/adv_surface_intersector`);
  the `ssi` crate and `solidcore-ssi`.
- Curve/surface intersection, as far as SSI uses it: the start points on the natural boundaries are
  boundary curves intersected with the other surface (§4.2). The API entry is
  `intersect::api_intersectors::intersect_curve_surface`, a global solve over the cache trees.
- What the boolean takes from SSI: `topology::face::p3::face_intersect` (classification of each curve
  against both faces) and the topology intersector (`boolean/topology_intersector`), which imprints the
  kept intervals as edges (§4.6).

**Out**, each with its own page:
- The blend's spine and rails, which are an offset/offset intersection traced with the blend's own
  events: [generic-blend.md](generic-blend.md).
- What a stored definition is, how a net is certified against it, and the certificate machinery
  (interval enclosures, the Krawczyk test) that §6 points to:
  [definitions-and-certificates.md](definitions-and-certificates.md).
- Sweeps stored as their definition: [procedural-sweep.md](procedural-sweep.md).
- Ray/body picking (`intersect/ray_tracer`), silhouettes (`intersect/surface_silhouette`) and point
  classification (`intersect/point_class`), which share the solver framework but not the curve
  problem.

## 3. The corpus and the measured gaps

This section is
[tangency-labs/solidcore#17](https://github.com/tangency-labs/solidcore/issues/17), condensed. Before
the `intersect.*` units were ported, the upstream C++ (SMLib at `c9c5979`) was measured on 119
surface pairs whose exact intersection is known by construction. The measurement is the ground the
rest of this page stands on: it shows which parts of the problem the marcher solves and which it does
not, and the corpus it produced is how every SSI change is graded since.

### 3.1 Setup

- **Build.** `source/SMLib` and `source/SM_API` only, at `-O3` with the upstream release defines, no
  TBB.
- **Entry points.** `SmSurface::GlobalSurfaceIntersect` (what `SmApiIntersectSurfaces` calls) is the
  primary one. Also measured: `SmAdvSurfaceIntersector`, `SmSurfaceIntersector`, `SmFace::FaceIntersect`
  (20°) and `SmApiIntersectSurfaces`.
- **Tolerances.** Approximation tolerances 1e-6 (primary), 1e-4 and the default; global tolerances at
  their defaults (zone 1e-5).
- **Failure sweep.** Every failure re-run in 26 configurations: 4 intersectors × 3 tolerances × both
  surface orders, plus the API in both orders. A trace build (`SM_DEBUG_CODE=1`) reported no
  invalid-input errors.
- **Ground truth.** Closed form for 116 cases. Three groups are traced numerically: the torus pairs,
  the tube arms and the appendix patch pair. Every stored point lies on both exact surfaces to 1e-9
  or better.
- **Scoring.** Coverage both ways (one-sided Hausdorff); exact distance to both surfaces; connected
  components and free ends; duplicated curves; whether every singular point is a curve end or an
  output point.

### 3.2 Results

**81 of 119 pass** in the primary run: 25 are incomplete, 11 have wrong topology, 2 return an error,
none crash. Every returned curve lies within 1.9e-6 of both exact surfaces, except two tangent curves
at 3.4e-5 (`y23_t2_ex4`, `y23_t6_ex1`).

| class | cases | pass |
|---|---|---|
| transversal (branches, loops, nested loops) | 14 | 14 |
| tangent along a curve | 10 | 7 |
| isolated tangent points | 7 | 3 |
| singular crossing points | 6 | 3 |
| patch boundaries, corners, shared borders | 8 | 8 |
| coincident regions | 7 | 7 |
| near-degenerate | 24 | 11 (9 failures are below the 1e-5 zone tolerance) |
| seams, poles, closed surfaces | 12 | 12 |
| high degree, many spans, rational | 6 | 6 |
| Yang, Jia and Yan [16], published data | 6 | 5 |
| Yang, Jia and Yan [16], reconstructions | 19 | 5 |

Below the 1e-5 zone tolerance, angles of 1e-6 to 1e-8, offsets of 1e-7 and loops of 2e-6 or less are
reported as such and not counted as defects.

**What the table says.** Every class in which the surfaces cross transversally passes: branches,
loops, boundaries, coincident regions, seams and poles, high degree. Boundary start points, one seed
per intersecting leaf pair and a Newton-corrected march (§4) are complete and accurate wherever
n1 × n2 is bounded away from zero. Every miss is at a contact where n1 × n2 = 0, or nearly: touch
points, tangent curves, branch points, tiny loops between nearly tangent surfaces. That is the
theory of this page in one line: **the hard part of SSI is the zero set of n1 × n2**, and each of
#17's four gaps is one way the marcher meets it (§4.4).

### 3.3 The four gaps

None of the 26 configurations closes them (unless a count says otherwise). The renders are #17's,
from `GlobalSurfaceIntersect` at 1e-6. In each, the top row is three 3D views with the exact
intersection drawn under SMLib's output and the missed parts marked; the bottom row is each surface's
parameter space and a close-up of the defect.

**Gap 1: isolated tangent points inside a patch are mostly missed.** The general path returns touch
points reliably only on patch boundaries; the sphere/plane and sphere/sphere special cases return
interior contacts. Missed: `c3n_parab_apex`, `c3n_parab_parab_apex`, `c3n_four_points`,
`c3_sphere_torus_outer`, `y23_fig1g` (both tangent contacts; the shared cone point at the apex is
found), `y23_t2_ex6`, `y23_t6_ex4`. Found: `y23_fig1b` (a contact between two transversal branches).
Why, in the code: `find_interior_curves` drops a leaf pair whose normals are within 5° at the seed or
2° at the solution (§4.2), which is every seed near a touch point.

![c3n_four_points: a quartic touching a plane at four interior points, none returned.](ssi/smlib-c3n_four_points.png)

*Look at the four interior contacts in the parameter-space panels: none is in the output. From #17.*

![y23_t6_ex4: an isolated tangential point, missed.](ssi/smlib-y23_t6_ex4_tangential_point.png)

*Look at the close-up: the single tangential point has no output point. From #17.*

**Gap 2: tiny loops above the zone tolerance are missed.** Loops of diameter 2e-4 and 2e-5 on a model
of size 2. On a NURBS paraboloid nothing is returned (2 of 26 configurations find the 2e-4 loop); on
the analytic sphere a single point is returned (`adv`/`basic` at 1e-6 find the 2e-4 loop, 4 of 26).
Why: every seed of a small loop between nearly tangent surfaces is below the same 5°/2° thresholds,
and the sphere's closed form took a small circle for a touch point.

![c7_tiny_loop_parab_1e-4: a loop of diameter 2e-4 on a NURBS paraboloid, missed.](ssi/smlib-c7_tiny_loop_parab_1e-4.png)

*Look at the close-up: the 2e-4 loop is there in the truth and absent from the output. From #17.*

**Gap 3: curves are not split at singular crossing points.** The point set is complete, but curves run
through the crossing without ending there: `c4_steinmetz` (1 of 2 crossings), `c4_viviani_figure8`,
`c4n_viviani_nurbs`, `y23_fig1a`, `y23_t6_ex3`, and the published `y23_t3_ex4` torus/ellipsoid pair.
Why: `compute_singularity_point` runs on start points, not on points the march passes, so the marcher
steps straight through a branch point (§4.4).

![c4_viviani_figure8: a sphere and a cylinder through its centre meet in a figure eight; the crossing is not a curve end.](ssi/smlib-c4_viviani_figure8.png)

*Look at the marked crossing: an output curve runs through it without ending. From #17.*

![y23_fig1a: two loops crossing at two points, not split.](ssi/smlib-y23_fig1a_two_loops_cross.png)

*Look at the two crossings of the loops: neither ends a curve. From #17.*

**Gap 4: tangent and high-order contact curves.**
- Open tangent curves come back twice, once in each direction: `c2n_cylN_on_planeN`, `y23_t6_ex1`
  (also 3.4e-5 off the surfaces), `y23_t2_ex4` (an inflected tangent curve, 58% overlap, 3.4e-5 off;
  7 of 26 configurations pass).
- Tangent curves collapse to their two end points: `y23_t2_ex1` (straight second-order contact) and
  `y23_fig1f` (second-order contact where the sheets cross).
- A closed tangent circle on a quartic is missed: `c2n_quartic_circle`.
- Partial tangent curves: `y23_t2_ex2` covered at 23% (6 of 26 pass, all at 1e-4); `y23_t2_ex3`, a loop
  and two curves meeting at four tangent nodes, covered at 8%; the tangential middle edge of
  `y23_fig1d` covered at 1%; one boundary curve of `y23_t6_ex6` covered at 88%, another returned seven
  times.
- `SM_ERR_NULL_POINTER` from `ReverseCurveDirection()`, called by `DoIntersection`:
  `c2n_quartic_contact_parabola` (fourth-order contact) and `y23_t2_ex5` (two loops, one tangential
  along its whole length) at 1e-6; at 1e-4 and the default tolerance `y23_t2_ex5` returns without
  error but incomplete.

Why: the predictor steps along n1 × n2, which is zero along a tangent curve, and the corrector
breaks down there; a tangent point is accepted as a tangent curve only when the principal curvatures
match (§4.4); each cluster of tangent boundary start points starts its own trace; and a trace that
comes back empty is reversed.

![c2n_cylN_on_planeN: a NURBS cylinder lying on a NURBS plane; the open tangent line comes back twice.](ssi/smlib-c2n_cylN_on_planeN.png)

*Look at the output list: the one tangent line is returned once in each direction. From #17.*

![y23_t6_ex1: a tangent line returned twice and 3.4e-5 off the surfaces.](ssi/smlib-y23_t6_ex1_tangent_line.png)

*Look at the close-up: the duplicated tangent line sits off the exact curve. From #17.*

![y23_t2_ex1: a straight second-order contact; the tangent curve collapses to its end points.](ssi/smlib-y23_t2_ex1_tangent_line.png)

*Look at the output: two end points where a whole tangent line should be. From #17.*

![y23_fig1f: a second-order contact where the sheets cross; the tangent curve collapses to its end points.](ssi/smlib-y23_fig1f_high_order_contact.png)

*Look at the contact curve: only its two ends are returned. From #17.*

![y23_t2_ex2: a tangential curve covered at 23%.](ssi/smlib-y23_t2_ex2_tangential_curve.png)

*Look at the coverage: the output follows under a quarter of the tangent curve. From #17.*

![c2n_quartic_contact_parabola: a fourth-order contact; the call fails with SM_ERR_NULL_POINTER.](ssi/smlib-c2n_quartic_contact_parabola.png)

*Look at the status line: the intersection fails instead of returning the contact parabola. From #17.*

![y23_t2_ex5: two loops, one tangential along its whole length; the call fails with SM_ERR_NULL_POINTER.](ssi/smlib-y23_t2_ex5_two_loops.png)

*Look at the tangential loop: nothing of it comes back at 1e-6. From #17.*

### 3.4 Reconstructions against the paper

Yang, Jia and Yan [16] publish no geometry for their Fig. 1, Table 2 and Table 6 examples, so #17
rebuilt them: each was fitted to the paper's figure through a camera recovered from the figure itself,
keeping the stated degrees, topology and contact type, with the exact intersection known in closed
form. An independent audit against the paper found 15 of 19 faithful in shape and topology and 4
matching in topology with a documented shape difference. The published-data cases are the Table 3/4
torus/ellipsoid pairs and appendix Example 2. Each image has three columns: the paper's figure, the
case rendered from the fitted camera, and the two overlaid; the top row is the paper's surfaces panel
and the bottom row its result panel.

![Reconstruction of Yang, Jia and Yan's Fig. 1a, two loops crossing.](ssi/fit-y23_fig1a_two_loops_cross.png)

*Look at the overlay column: the rebuilt surfaces and loops sit on the paper's figure. From #17.*

![Reconstruction of Fig. 1f, a high-order contact.](ssi/fit-y23_fig1f_high_order_contact.png)

*Look at the result panel: the rebuilt contact curve matches the paper's. From #17.*

![Reconstruction of Table 2, example 5, two loops.](ssi/fit-y23_t2_ex5_two_loops.png)

*Look at the overlay: both loops, the tangential one included, match the paper's. From #17.*

![Reconstruction of Table 6, example 6, four boundary curves.](ssi/fit-y23_t6_ex6_four_boundary_curves.png)

*Look at the four boundary curves in the result panel against the paper's. From #17.*

### 3.5 The proposal, and what came of it

#17 proposed three steps:
1. **The corpus becomes fixtures for the `intersect.*` units**: per case, the two surfaces, the exact
   truth (implicit or parametric) and the expected branches, isolated points, singular points and
   coincident regions with a coverage tolerance. The unit runners use only the surfaces, and
   their goldens record the results.
2. **A separate ground-truth acceptance test** that scores the port's output against the exact truth
   per case, and does not gate a unit's green status (a green unit must reproduce the gaps).
3. **Fixes after parity**: each gap its own change, gated by the acceptance test going from fail to
   pass with no regression elsewhere in the corpus.

What came of it, from #17's thread:
- The corpus is public at [jyviko/ssi-corpus](https://github.com/jyviko/ssi-corpus) (MIT OR
  Apache-2.0): the 119 case files with exact truth, the generators, the scorer, and the SMLib runner
  as the runner contract's example.
- `goldens --ssi-corpus` is step 2. It scored the C++ at 81/119, matching #17, and its scorer
  agrees with the corpus's `score.py` on 119/119 verdicts (§9). The 38 C++ misses are logged as
  WR-74 to WR-80, the fixes of §4.4.
- #17 also recommends the method of Yang, Jia and Yan [16] for completeness, since neither SMLib nor
  SolidCraft's earlier kernel completed the cases reported in the literature.
- Step 3's policy has since been replaced: fixes change the default result, with no compatibility
  flag, and the existing `UpstreamFixes` flags are to be retired (AGENTS.md §4b, 2026-11-04). The
  WR-80 fix is the default; the WR-74 to WR-79 fixes still sit behind flags (§4.4), which is
  open work.

## 4. The pipeline

The problem: given S1(u, v) and S2(s, t), find the set S1(u, v) = S2(s, t) inside both domains, as
curves and isolated points, each curve with both pcurves. In four unknowns with three equations the
generic solution is a curve; where the normals n1 and n2 are parallel, the system loses rank and the
curve may stop, branch, or widen into a region [11].

Every method has the same three parts [11, 1]: find at least one point on every component (start
points), follow each component (marching), and handle the points where following fails (tangency,
branch points, coincidence). Subdivision alone is complete but slow and fragments curves; marching
alone is fast and accurate but can miss closed loops [2]. Methods that find the topology first, from
the critical points of the problem, are the other family [7, 5]. Both implementations here subdivide
for completeness and march for accuracy.

![Two surface patches meeting in a curve: subdivision boxes, start points on the boundary and in the interior, and the march with its predictor and corrector.](ssi/march.svg)

*Figure 1. Look at the start points: one on S1's boundary for C, one inside a leaf pair for the
closed loop L; from each, the march predicts along n1 × n2 and corrects onto both surfaces.*

### 4.1 Dispatch: special cases first

`Surface::global_surface_intersect` (`solidcore/src/surface/surface/p2.rs`) matches on the first
surface's kind. A plane, cone (cylinders are cones of zero semi-angle), sphere or torus tries its
closed form against the other surface's kind first (§5). Each special case returns its curves and a
flag saying whether the general intersector is still needed; when it is, `do_intersection`
(`intersect/surface_intersector/p5.rs`) runs on an `AdvSurfaceIntersector` and resets the output
first. Any other pair, and every B-spline, procedural blend, offset or curve-bounded surface, goes
straight to the general intersector.

`ssi` orders its own paths the same way (`ssi/src/intersect.rs`, `intersect_bodies`): closed forms for
a plane against a sphere or a degree-2 form (`quadric`), then the field path where one side has an
implicit equation (`trace2d`), then subdivision and marching, then local Taylor fields near
tangencies (`taylor`).

### 4.2 Start points

In the port (`do_intersection_with_caches`, `intersect/surface_intersector/p5.rs`):
1. **Boxes.** If the root boxes of the two surface caches' Bézier decomposition trees are apart by
   more than the tolerance, there is no intersection. Otherwise a global solve
   (`SM_SO_INTERSECTION_TEST`) collects the leaf pairs whose patches may meet.
2. **A thin box.** One leaf pair whose box is thinner than twice the tolerance in some dimension goes
   to `grazing_box_process`: a corner pair of the two domains closer than the tolerance, with
   tangents that do not point into each other, is a touching point and ends the search.
3. **Boundary planes.** `do_boundary_plane_test` (`p2.rs`) compares the planar boundaries of the two
   control nets: every sample of each boundary beyond the other's plane proves the surfaces apart;
   a pair just touching is a graze, answered by intersecting the two boundary curves.
4. **Boundary start points.** `find_boundary_start_points` (`p3.rs`) intersects each surface's
   natural boundary curves with the other surface. Each start point is traced (§4.3), backwards too
   when the curve is open and starts along a boundary.
5. **Interior start points.** `find_interior_curves` (`adv_surface_intersector.rs`) takes one
   candidate per intersecting leaf pair from the global solve and traces it when the normals are at
   least 5 degrees apart at the guess and 2 degrees at the solution, and the point is not on a curve
   found already. This is the loop detector of the port: a closed loop is found when some leaf pair
   seeds it.

The 5 and 2 degree thresholds are where interior loops go missing: a small loop between nearly
tangent surfaces has every seed below them. `ssi_low_angle_start_on_probation` (WR-79) traces such
seeds on probation and keeps the trace only as a closed crossing loop larger than twice the tolerance.

In `ssi` (`ssi/src/intersect.rs`, `piece.rs`) completeness is proved instead of seeded:
1. each B-spline is cut into rational Bézier pieces by knot insertion [12];
2. piece pairs are halved until each is **apart** (boxes, or slabs across a normal cone) or
   **loop-free**: their normal cones hold no parallel pair, so no curve closes inside the pair and
   every curve has both ends on the pair's boundary (Sederberg and Meyers's loop test [13]);
3. in a loop-free pair, the boundary curves of each piece are intersected with the other piece:
   those points are the seeds, and they reach every arc.

A pair still neither apart nor loop-free when both pieces are smaller than `Options::resolution`
(1e-5) is a tangency, a coincidence or a degenerate piece at the crossing, and is refused by name
unless the field or Taylor path resolves it (§4.4).

### 4.3 Marching

Port (`intersect/surface_intersector/p3.rs`, `compute_next_point`; `p4.rs`, `trace_curve`):
- **Predictor.** A step along the point's UV tangents, the pullbacks of n1 × n2, clipped to both
  domains. The first step is `100 × approx_tol_3d` (`compute_step_size`, `p1.rs`).
- **Corrector.** `refine_point` (`p1.rs`) solves back onto both surfaces, at the approximation
  tolerance for crossing points and ten times that at tangent and singular points
  (`Surface::local_surface_intersect`, `surface/surface/p11.rs`, with a plane through the guess).
- **Span test.** `test_span_accuracy` (`p1.rs`) accepts the step when the tangent turns by less than
  the angle tolerance and, at five interior parameters, the 3D Hermite span and both UV Hermite spans
  mapped onto their surfaces agree within `approx_tol_3d`. A rejected span halves the step; a span
  well inside both limits (an eighth of the tolerance, a quarter of the angle) grows it by 1.5.
- **End.** The trace stops at a domain boundary, at a through, start or end point it snaps to
  (`test_point`), on reaching a curve already found, or after 5000 points (an error).
- **Output.** `flush_curve` (`p2.rs`) writes the points as a piecewise cubic C0 B-spline in 3D and
  in each UV domain, one knot vector for all three, with the curve type and the largest deviation
  seen.

`ssi` marches each arc of a loop-free pair from its seed to its exit with a predictor along
nA × nB and a Newton corrector held to a plane across the curve; Newton stops at a residual of
`Options::tolerance` (1e-11) scaled by the sine of the angle between the surfaces, with steps of at
most `max_step` (0.02) and turns of at most `max_turn` (0.06 rad). Arcs are oriented along nA × nB
and joined end to start when their ends are within `link` (1e-7). Orientation: with both bodies'
normals pointing out, the part of A inside B is to the left of the curve seen from outside A.
`solidcore-ssi/src/curves.rs` turns the points into one 3D and two UV cubic Hermite curves on one
knot vector, the 3D tangent along n1 × n2 and each UV tangent its pullback through that surface's
Jacobian, and adds points on every knot line crossed and in spans whose measured deviation is over
the tolerance (up to six rounds). Near a domain edge or a pole the UV tangents are limited so each
coordinate stays monotone where its points are (Fritsch and Carlson's condition [4]).

### 4.4 Turning points and singularities

Where n1 ∥ n2 the marching direction n1 × n2 vanishes. The local picture is the difference of the two
surfaces' second fundamental forms in the common tangent plane, read through the two Dupin
indicatrices [11]: if the difference is definite the contact is an isolated **touch point**;
if it is indefinite, two branches cross at a **branch point** (both surfaces cross along two curves);
if it is semidefinite, the surfaces may be tangent along a **tangent curve**; if it vanishes
identically, the contact is of higher order or the surfaces **coincide** there.

![Left: two surfaces touching along a tangent curve, normals equal along it. Right: a branch point where two intersection branches cross, with the four directions leaving it.](ssi/branch-point.svg)

*Figure 2. Look at where the normals are parallel: along the whole tangent curve (left) there is no
marching direction; at the branch point B (right) there are four, and both branches end there.*

**Port.** `AdvSurfaceIntersector::compute_singularity_point` (`adv_surface_intersector.rs`) classifies
a point with parallel normals from the two Dupin indicatrices: no common direction is a tangent
point, two a tangent curve, more a singularity whose directions are the indicatrices' intersections
(preferring one into both domains at a start, else the one closest to the previous direction).
Directions count as one only when they are the same one, to about 0.08°: upstream's 25.8° window read
two close branches as a tangent curve (#2695).
Equal umbilical curvatures are a coincidence. It runs on start points and on points the corrector
lands on, not on every point the marcher passes, so the base marcher steps straight through a branch
point. The fork's fixes, all off by default:

| flag (record, issue) | what it adds | file |
|---|---|---|
| `ssi_split_curves_at_singular_points` (WR-77, #393) | samples \|n1 × n2\| along each crossing curve, refines each sharp minimum by golden section, and splits the curve where the sine is below 1e-6 and rises above 1e-3 on both sides, more than ten approximation tolerances from its ends | `surface_intersector/singular_splits.rs` |
| `ssi_tangent_curve_tracer` (WR-75, #395) | traces tangent curves the marcher cannot follow: Levenberg-Marquardt on S1 − S2 = 0 with n1 ∥ n2, directions probed on a small circle in the tangent plane; traces crossing branches through tangent contact nodes and reports the nodes | `surface_intersector/tangent_curves.rs`, `tangent_tracer.rs` |
| `ssi_tangent_contact_points` (WR-74, #394) | Newton on a 4 × 4 system (the gap along S2's tangents, S1's tangents against n2) from the leaf pairs the interior search skips, plus the poles; a root is a touch point when the gap is below 1e-7 of the tolerance and the normals agree to 1e-4 rad | `adv_surface_intersector/tangent_contacts.rs` |
| `ssi_trace_tangent_curves_once` (WR-76, #392) | one start point per cluster of tangent boundary start points, so an open tangent curve is traced once | `surface_intersector/p5.rs` |
| `ssi_singular_start_inside_direction` (WR-512) | a curve starting at a singularity leaves along a direction into both domains | `adv_surface_intersector.rs` |
| `ssi_pole_point_keeps_its_tangent` (WR-550) | a point at a pole keeps its exact limit tangent when stepping back | `surface_intersector/p1.rs` |

**`ssi`, the field path** (`ssi/src/trace2d.rs`, `field.rs`). Where one patch lies on a surface with
an implicit equation f (an analytic surface, or a B-spline patch whose exact implicit form
`algebraic.rs` finds, §6.1), the intersection is the zero set of g = f(P(s, t)) on the other patch's
own parameters. On a rational piece, g is a tensor Bernstein polynomial, W^k f(X / W), with the sign
of f, and its Bernstein coefficients bound it there [3, 14]. Each piece's square is cut into cells:
- g of one sign: no intersection there, proved by the coefficients;
- ∂g/∂t of one sign: the zero set is a graph t(s), each point a bracketed root, so a trace cannot
  jump branches (and alike with s and t swapped);
- neither: the cell may hold a critical point of g. When the Hessian's determinant has one sign over
  the cell and ∇g winds once round its boundary, the cell holds exactly one nondegenerate critical
  point: an extremum at zero is a touch point, a saddle at zero is a branch point;
- where g is convex along one parameter's lines, or its third or fourth derivative along them has
  one sign, the curve of its extrema is a tangent curve, two branches, or none, proved on its chords;
- otherwise the cell is halved, down to the resolution, where nothing proved is refused.

Where neither patch has an implicit form, `taylor.rs` builds a local field: the separation λ(s, t) of
A from B along a fixed direction, a Taylor polynomial with proved bounds on its value's, gradient's
and Hessian's errors, read by the same cell tests. The polynomial proves; every point is still solved
on the surfaces themselves.

`ssi` joins branches straight through their crossings and reports the crossings as vertices beside
the curves (`Intersection::vertices` in `solidcore-ssi`); a B-rep builder splits its edges there.

### 4.5 Closed loops and coincident regions

A closed loop has no boundary start point. The port finds it from a leaf pair's seed (§4.2, step 5)
and closes the trace when it returns to its own start. `ssi` cannot miss it: in a loop-free pair a
loop cannot close, so subdivision halves until every loop crosses some pair's boundary and is seeded
there.

Coincident patches share a region, and the region's boundary is the intersection
(`TsectCurveType::RegionBoundary`). The port answers parallel coincident planes, coaxial surfaces of
revolution and parallel cylinders in their closed forms (§5); a general coincident pair leaves the
marcher with parallel normals everywhere. `ssi/src/coincide.rs` reads coincidence off the composed
field (every coefficient zero), then builds the boundary from each patch's edges inside the other,
deciding inside by inverting each edge point onto the other patch and comparing parameters, never
distances. In the boolean, faces known coincident are handled before SSI: `face_intersect` runs the
coincidence check first and returns the faces' own edge curves trimmed to each other.

### 4.6 What the boolean takes from SSI

`face_intersect` (`topology/face/p3.rs`) calls `global_surface_intersect_with_context` over the two
faces' UV domains with the curve angle tolerance, then for each curve:
- a crossing curve that bounds a cap within the tolerance is a touch at the B-rep's tolerance: at each
  of eight points the surfaces' crossing angle θ over twice the curve's curvature κ (the cap's height
  to second order: r² / 2R for a sphere cut in a circle of radius r) is at most the tolerance, its
  centre of curvature lies within the tolerance of both surfaces (which a tangent curve's does not),
  and the curve becomes a touch point at that centre, once per cap (`cap_centre_within_tolerance`,
  WR-79, #2412). An edge there would join faces within tolerance of each other across a band
  tol / θ wide, wider than the cap, which the merge pairs as coincident;
- a degenerate curve (a touch point) is classified at its point on both faces and kept as a point when
  it is on or inside both;
- every other curve is classified on each face (`curve_on_classify_mut`), with its UV curve when SSI
  gave one, and the intervals inside or on both faces are kept.

The topology intersector (`boolean/topology_intersector/p1.rs`, `face_face_intersect`) imprints each
kept interval as an edge in each face and glues the two. The boolean consumes the 3D curves, the UV
curves (which become the imprinted edges' trims, #489), the curve types and the points. It does not
read `deviations`. A curve from a closed form has no UV curves (`surface/cone/p3.rs`, `add`: `None`
on both sides, deviation 0); its trims are made later by dropping the 3D curve onto each surface
(`Surface::drop_curve`, `intersect/surface_drop_lib`), which traces and fits a cubic within the drop
tolerance. So an exact edge can carry fitted trims; §5 lists the exact pcurves that replace them.

## 5. The exact classes

A pair of natural quadrics (plane, sphere, circular cylinder and cone) meets in a curve of degree at
most 4, and in conics and lines for the configurations that are classically degenerate: a plane with
any quadric, coaxial quadrics, parallel cylinders, and equal cylinders with meeting axes [9, 8].
These are the pairs solved in closed form; every other pair is traced.

| pair | configuration | result | code |
|---|---|---|---|
| plane/plane | not parallel | a line, or its segment in both domains | `surface/plane/p2.rs`, `Plane::intersect_with_plane` |
| plane/plane | parallel apart; coincident | nothing; the general intersector (coincident) | same |
| plane/cylinder | parallel to the axis | two lines, one tangent line, or nothing, from \|d\| against R | `surface/cone/p3.rs`, `Cone::intersect_with_plane` |
| plane/cylinder or cone | square to the axis | a circle | same, `Plane::make_new_plane_ellipse` |
| plane/cylinder | oblique | an ellipse | same |
| plane/cone | oblique, steeper than the semi-angle | an ellipse | same |
| plane/cone | through the apex, along the axis | two lines | same |
| plane/cone | parabola or hyperbola section, or parallel off-axis | the general intersector | same |
| plane/sphere | \|d\| < R | a circle; a touch point at \|d\| = R | `surface/sphere.rs`, `Sphere::intersect_with_plane` |
| sphere/sphere | not concentric | a circle, or a touch point | `Sphere::intersect_with_sphere` |
| sphere/cone | centre on the cone's axis | circles of the cone | `Sphere::intersect_with_cone` |
| cylinder/cylinder | parallel axes | lines from the circle/circle section | `Cone::intersect_with_cone` |
| cylinder/cylinder | equal radii, axes meeting | two ellipses | same |
| plane/surface of revolution | plane through the axis; square to it | the generator rotated in; circles | `surface/surf_of_revolution/p1.rs` |
| revolution/revolution | same axis, planar generators | circles at the generators' intersections | `surf_of_revolution/p3.rs` |
| plane/extrusion, extrusion/extrusion or cylinder | extrusion parallel to the plane or to the other | lines along the extrusion | `surface/surf_of_extrusion/p2.rs` |
| plane/torus | square to the axis; through it | circles; meridian circles | `surface/torus.rs` (plane side: `plane_first_torus_section_exact`, WR-721) |

How each is made exactly: the curve's defining data (a centre, axes and radii, or a line's points)
are computed from the two surfaces' frames, and the curve is stored as its rational form (an
`Ellipse`, a circle or a line), then trimmed to the domains. Nothing is sampled.

![A plane cutting a cylinder obliquely in an ellipse; the plane's normal, the cylinder's axis and the angle between them; the ellipse's centre and semi-axes.](ssi/plane-cylinder.svg)

*Figure 3. Look at the angle θ between the axis and the plane's normal: it alone decides whether the
exact section is a circle, an ellipse with semi-axes R and R / cos θ, or lines along the axis.*

**Plane/cylinder in detail** (`Cone::intersect_with_plane`). With a the axis, n the plane's normal and
θ their angle:
- θ within `100 · EFF_ZERO · 360` of 90° (plane parallel to the axis): d is the distance from the
  axis to the plane. |d| > R + tol gives nothing; |d| ≥ R, or two lines closer than the tolerance,
  is one `Tangent` line; otherwise two `Crossing` lines at ±√(R² − d²) across the axis, each kept if
  its angle lies in the cylinder's sweep. With `cylinder_plane_tangent_within_tolerance` (WR-510,
  #1525), |d| > R − zone tolerance of either face is also tangent, so a plane a hair off a wall does
  not cut it in two lines microns apart.
- θ near 0 (plane square to the axis): the circle of radius R about the axis point on the plane.
- otherwise: the ellipse, centre where the axis meets the plane, built from the two generator lines
  in the plane spanned by a and n.

**`ssi`'s closed forms** (`ssi/src/quadric.rs`). A plane and a sphere meet in the circle about the
foot of the centre of radius √((R − |d|)(R + |d|)), d a compensated dot product, so the difference
R − |d| is exact where they are close [15] and a loop of radius 1e-7 keeps its radius
(`c7_tiny_loop_sphere_1e-7`). A plane and a patch whose exact implicit form has degree 2 meet in the
conic of the form restricted to the plane; where that conic's quadratic part is definite it is an
ellipse, sized from one evaluation of the form at its centre, told from rounding by the form's own
bound (`c7_tiny_loop_parab_1e-7`). Other conics take the field path.

**Exact pcurves.** A plane section on a cylinder has an exact UV curve: on one rational quadratic
segment of the cylinder's circle, s / (1 − s) is a ratio of affine functions of the point, so a
rational curve of degree p on the cylinder has a rational parameter s(t) of degree p and an axial
height that is a polynomial over its weight. `surface_drop_lib/cylinder_section.rs` builds it in the
curve's own parameter with twice its degree (WR-680); a curve it does not apply to is traced. On a
plane every curve's UV image is the same curve in the plane's frame.
Other exact pcurves (sphere, cone, torus sections) are open work.

## 6. Certification and tolerances

### 6.1 What is certified today

| object | port | `ssi` |
|---|---|---|
| a closed-form curve (§5) | exact by construction, deviation 0 | exact by construction |
| a point on a traced curve | a Newton root within `approx_tol_3d` | a Newton root within 1e-11 × sine of the angle, or a bracketed root of the field |
| "no intersection here" | box and boundary-plane tests | boxes, slabs, normal cones, field coefficients of one sign |
| "this cell has one touch or crossing" | not proved | Hessian sign plus gradient winding (degree theory) |
| a tangent curve | not proved | the field's valley proved on its chords |
| an implicit form for a B-spline | none | composed with every piece, every coefficient within 1e-11 of the terms' scale (`algebraic::PROOF`) |
| the curve between its points | five interior samples per span (`test_span_accuracy`) | `solidcore-ssi` measures the deviation at its points and three interior points a span; "not yet a proved bound" (its README) |

So both implementations prove where the curve is (the points) and `ssi` proves the topology, but
neither proves the B-spline between its points. The certificate the one rule asks for is the one the
blend's net carries ([generic-blend.md](generic-blend.md) §2.4,
[definitions-and-certificates.md](definitions-and-certificates.md)): the curve's error against the
intersection, known at the span's grid points to machine precision and bounded between them by the
interpolation remainder with derivatives enclosed over the span box, with the root's existence and
uniqueness in the box proved by the Krawczyk test [10]. The intersection curve is a definition in
Hoffmann's sense [6]: the system S1(u, v) − S2(s, t) = 0 with a normalizing equation, evaluated on
demand. A sampled check is not a certificate; where interval bounds over-estimate, the remedy is a
tighter enclosure, never a sample.

### 6.2 Tolerances

| name | value | where | used for |
|---|---|---|---|
| `zone_tol_3d` | 1e-5 | `Kernel::tolerances` (`kernel.rs`), `geom/tol.rs` | points closer than this are one point; the B-rep's base tolerance |
| `xsect_tol_3d` | 2e-5 | same | default gap and intersection tolerance |
| `approx_tol_3d` | 5e-6 | same | default curve approximation tolerance; the corrector's and the span test's tolerance |
| `ang_tol_deg` | 0.5° | same | the span test's turn limit by default |
| `EFF_ZERO`, `EFF_ZERO_SQRT` | 1e-12, 1e-6 | `math.rs` | zero tests; the closed forms' tolerance when none is given |
| a face's tolerance | per face | the B-rep | plane boxes in the closed forms are expanded by it (`PlaneOwner`) |
| interior seed angles | 5°, 2° | `adv_surface_intersector.rs` | below them a leaf pair's seed is skipped |
| corpus approximation tolerance | 1e-6 | `goldens --ssi-corpus` | the tolerance passed to each engine |
| corpus accuracy tolerance | 1e-5 (10 × tol) | `goldens/src/ssi/score.rs` | a curve farther than this from an exact surface is `INACCURATE` |
| `ssi` Newton | 1e-11 | `ssi::Options::tolerance` | residual, scaled by the sine of the angle |
| `ssi` link, resolution | 1e-7, 1e-5 | `ssi::Options` | arc ends joined; pieces below this unresolved are refused |
| `ssi` field band | 1e-14 of the field's scale | `trace2d.rs`, `CLEAR` | a value within it is zero to rounding |
| `ssi` implicit form | 1e-13, 1e-11 | `algebraic.rs`, `EXACT`, `PROOF` | least singular value at rounding; proof by composition |

The zone tolerance is the B-rep's: two points, edges or faces within it are one. An intersection
curve must lie within the tolerance of both faces it joins, and its edge's tolerance grows to its
largest deviation; a curve whose own error is a large fraction of the zone tolerance leaves no margin
for the edges and vertices built on it (the gaps of #2537 and #2538 are 7e-6 to 1e-5 against
tolerances of 7.5e-6). That is why the general curve's bound should sit far below the zone tolerance,
and why the approximation tolerance is half of it.

## 7. Failure modes

| mode | what goes wrong | port today | `ssi` | open |
|---|---|---|---|---|
| tangent curve (#17 gap 4) | n1 × n2 = 0 along it; the marcher has no direction | collapses to its end points (#395); fixed with `ssi_tangent_curve_tracer` | a proved valley of the field, or of a Taylor field | flag on by default |
| touch point inside both faces (#17 gap 1) | no boundary start, normals within 5° at every seed | missed (#394); found with `ssi_tangent_contact_points` | a proved extremum of g at zero | flag on by default |
| branch point (#17 gap 3) | the marcher steps straight through; one curve where two end | wrong topology (#393); split with `ssi_split_curves_at_singular_points` | a proved saddle; branches joined through it, vertex reported | #393 |
| small loop, nearly tangent (#17 gap 2) | every seed below the 5°/2° thresholds | missed (#396); found with `ssi_low_angle_start_on_probation` | loop-free pairs cannot hide it; closed form for plane/sphere and degree-2 | flag on by default |
| small circle taken as a point | gap to tangency within tolerance though the circle is not | fixed (WR-79): the sphere's closed forms collapse a circle only when its radius is within the tolerance; `FaceIntersect` takes a circle whose cap lies within the tolerance as a touch (§4.6) | the radius from √((R − \|d\|)(R + \|d\|)) | |
| coincident faces | normals parallel everywhere | closed forms for planes, coaxial revolutions, parallel cylinders; else the boolean's coincidence check before SSI | region boundary from the field's zero coefficients | |
| near-coincident strip | two planes within tolerance across a strip; the crossing is fixed only to tol / sin θ | `boolean_coincident_strip_drops_crossing` (WR-614) | patches 1e-8 apart, separating only at second order: refused (`y23_fig1d_two_quads`) | |
| grazing | boxes just touch; a boundary meets the other surface tangentially | `grazing_box_process`, `do_boundary_plane_test` | the pair is apart or refused | |
| plane a hair off a cylinder wall | two lines microns apart instead of one tangent line | WR-510, `cylinder_plane_tangent_within_tolerance` | | flag on by default |
| near-miss seam | an intersection runs within tolerance of a closed surface's seam | the curve and its trims end near but not on the seam: `SmFace::Test #36` "Face Near missing Seam", or `make_faceuse` 1001 | curves split at the seam by `solidcore-ssi` | #2538, #2537, cause not traced |
| curve across a pole | the UV coordinate along a collapsed edge names no point | `ssi_pole_point_keeps_its_tangent` | split at the pole; placed by its approach; the pole divided out of the field | |
| empty trace reversed (#17 gap 4) | `SM_ERR_NULL_POINTER` | not traced back: a trace that produced no points is not reversed (WR-80, `surface_intersector/p4.rs`, `p5.rs`) | | `adv_surface_intersector.rs`'s own trace-back is unchecked; no known case reaches it (WR-80 addendum 2026-11-05) |
| trace that never ends | 5000 points, or a creeping boundary trace | an error at 5000; `ssi_start_loop_trace_budget` bounds all start points | `Error::Unfinished` at 100,000 steps; pair budget 2,000,000 | |

**Refusals.** `ssi` refuses by name: `BOOLEAN_TANGENT` (a contact it cannot prove), `BAD_PATCH`,
`MARCHING_STALLED`, `EXIT_DIVERGED`, `ARC_UNFINISHED` (`ssi/src/lib.rs`). A refusal names where. The
port returns `SolidError` statuses; a refusal there is an error code, and an unported callee is
`SolidError::Unported`, which the corpus report scores as `DEFERRED`, never as a result.

**Near-miss seams (#2538, #2537).** A box cut by a cylinder tilted 4° whose axis lies in a box face, or
whose end cap is bisected by a face, fails the B-rep check with `SmFace::Test #36` or a 1001 in
`make_faceuse`; turning the cylinder's seam away from the material, the mirror image, or a cylinder
1 µm longer builds. The intersection curves pass within the zone tolerance of the cylinder's seam
line and of the cap's edge. The cause has not been traced; the place to look is where the
intersection's curve meets the seam (§4.6: the curve's trims and the seam split) rather than the
check. Whatever the fix, an intersection curve crossing a closed surface's seam must be split on the
seam with both pieces' trims in the domain, which `solidcore-ssi` does and the port leaves to the
drop.

## 8. Invariants

A reviewer can check each of these on a change to SSI.

1. A pair of §5 is answered in closed form: the 3D curve is a line, circle or ellipse stored as
   such, with deviation 0.
2. No point of an output curve is chosen by thresholding distances at sample points; every point is a
   root of the intersection system or a bracketed root of a field.
3. No region is dropped except by a proof: disjoint boxes or slabs, a field of one sign, a boundary
   plane test.
4. Every output curve lies within its reported deviation of both surfaces, and its UV curves map onto
   its 3D curve within that deviation.
5. The 3D curve and its UV curves share one parameter and one knot vector.
6. Every UV curve stays inside its surface's domain; a curve crossing a closed surface's seam is split
   there.
7. A curve ends at every branch point it reaches, and the branch point is reported as a point.
8. A touch point is a degenerate curve of type `Touching` (or an isolated point), never dropped and
   never a short curve.
9. Every closed component is found: a loop inside both domains has a seed, or its absence is proved.
10. Coincident regions come back as their boundary, typed `RegionBoundary` or `Coincident`.
11. No curve is reported twice, and no degenerate curve lies on another curve.
12. A contact that cannot be resolved is refused by name with where; nothing is approximated to get
    past it.
13. The same input gives bit-identical output, independent of thread count.
14. A change's tests check values a wrong result changes: corpus verdicts, distances to the exact
    curve, counts of curves and points, exact volumes in the boolean. Never timings.
15. A change to ported code that is not a fix keeps the goldens of its `intersect.*` units bit
    for bit; a fix changes the default result, blesses the goldens it moves and lists what changed
    (AGENTS.md §1, §4b).

## 9. Status

**The SSI corpus.** `goldens --ssi-corpus` scores the engines on jyviko's corpus
([ssi-corpus](https://github.com/jyviko/ssi-corpus), commit `b04c8a6`): 119 cases (planes,
quadrics, tori, B-spline and NURBS pairs, 25 from Yang, Jia and Yan [16]), each with the dense exact
intersection and its topology, at approximation tolerance 1e-6 (`crates/goldens/README.md`).

| engine | pass (of 119) | not passing | source |
|---|---|---|---|
| C++ reference (the pinned SMLib) | 81 | 25 incomplete, 11 wrong topology, 2 error | `reports/ssi-corpus.json` (2026-10-22) |
| the port, default flags | 78 | 21 incomplete, 10 wrong topology, 1 error, 9 deferred | same; parity with the C++ 110 of 110 compared |
| `ssi`, points as marched | 118 | 1 refused (`y23_fig1d_two_quads`) | `crates/ssi/README.md` |
| `solidcore-ssi`, B-spline curves | 118 | the same refusal | `crates/solidcore-ssi/README.md` |

The C++ row reproduces #17's primary run (§3.2). The port's column is the pinned behaviour:
 its fixes (§4.4, §7) are off in the corpus run. The one
case no engine passes, `y23_fig1d_two_quads`, is two patches 1e-8 apart that separate only at second
order in places, below the 4.4e-13 their degree-9 form is exact to.

**Port units.** The goldens units are `intersect.surface_intersector_p1` to `_p5`,
`intersect.adv_surface_intersector`, `intersect.surface_tracer`, `intersect.surface_drop_lib_p1` to
`_p3`, `intersect.surface_drop_curve`, `intersect.curve_class_p1` to `_p9`, `intersect.point_class`,
`intersect.topology_solver_p1` and `_p2`, `intersect.api_intersectors`, `intersect.ray_tracer`,
`intersect.surface_silhouette` (`plan/units/`), and `boolean.topology_intersector_p1` to `_p3` for the
boolean's side.

**Open issues.**

| issue | topic |
|---|---|
| [#17](https://github.com/tangency-labs/solidcore/issues/17) | the corpus and the measured gaps of the upstream intersector |
| [#391](https://github.com/tangency-labs/solidcore/issues/391) | an empty trace reversed (`SM_ERR_NULL_POINTER`); fixed on the default path |
| [#393](https://github.com/tangency-labs/solidcore/issues/393) | curves not split at singular crossing points; fix behind a flag |
| [#395](https://github.com/tangency-labs/solidcore/issues/395) | tangent and high-order contact curves missed or collapsed; fix behind a flag |
| [#1552](https://github.com/tangency-labs/solidcore/issues/1552) | how `solidcore-ssi` is wired into the kernel |
| [#2479](https://github.com/tangency-labs/solidcore/issues/2479) | pcurves end one ulp past their face's domain |
| [#2537](https://github.com/tangency-labs/solidcore/issues/2537) | a tilted cylinder whose cap is bisected by a box face is refused (#36) |
| [#2538](https://github.com/tangency-labs/solidcore/issues/2538) | a notch cylinder along a slanted box edge fails (1001, #36) |

**Work that has no issue yet**, to be filed before it is claimed: a certified bound for general
intersection curves (§6.1); exact pcurves for sphere, cone and torus sections; the merge of the two
implementations ([layers.md](layers.md) §2, step 4 of its order).

## 10. If you're stuck

- **Reproduce at the surface level first.** Pull the two faces' surfaces and domains out of the
  failing operation and call `Surface::global_surface_intersect` on them alone. A boolean failure
  whose intersection is right is a classification or imprint bug (§4.6), not an SSI bug.
- **Run the same pair through `ssi`.** `solidcore_ssi::intersect` takes the same inputs
  (`Engine::Ssi`, `Engine::Port`). If `ssi` gets it and the port does not, compare curve counts,
  types and end points; if both disagree with your expectation, check the expectation.
- **Find the case's class in #17's table** (§3.2) and its gap (§3.3). A transversal pair that
  fails is rare and worth a corpus case; a contact pair is one of the four gaps.
- **Check the class.** Is the pair in §5? Then a traced curve is the wrong path: find why dispatch
  missed the closed form (a NURB that lost its analytic identity, a cone that is really a cylinder,
  a frame off by rounding).
- **Look at n1 × n2 along the curve.** Near zero anywhere means §4.4: a tangency, a branch point, a
  coincident patch. Most wrong-topology results are here.
- **Look at the seam and poles.** A curve within the zone tolerance of a closed surface's seam, or
  through a pole, is §7's near-miss seam or pole row.
- **Check the flags.** The port's fixes are off by default; SolidCraft runs with most of them on. A
  result that differs between the two is usually a flag (`UpstreamFixes::flags` lists them).
- **Use the corpus.** `goldens --ssi-corpus 'c3_*'` runs a class; the per-case JSON in
  `reports/` has each verdict's distances and topology signature. Add a corpus-style case for your
  pair when none exists.
- **Read the literature before designing.** The survey chapters of [11] and the papers below cover
  nearly every failure here.
- **Fix the cause.** No retry with a smaller step, no second method tried after the first fails, no
  loosened tolerance. A failure that several cases share has one cause.

## 11. References

Each entry was checked against its publisher's record; the DOI is given.

1. R. E. Barnhill, G. Farin, M. Jordan, B. R. Piper. Surface/surface intersection. *Computer Aided
   Geometric Design* 4(1–2) (1987) 3–16. doi:10.1016/0167-8396(87)90020-3
2. R. E. Barnhill, S. N. Kersey. A marching method for parametric surface/surface intersection.
   *Computer Aided Geometric Design* 7(1–4) (1990) 257–280. doi:10.1016/0167-8396(90)90035-P
3. R. T. Farouki. The characterization of parametric surface sections. *Computer Vision, Graphics,
   and Image Processing* 33(2) (1986) 209–236. doi:10.1016/0734-189X(86)90115-5
4. F. N. Fritsch, R. E. Carlson. Monotone piecewise cubic interpolation. *SIAM Journal on Numerical
   Analysis* 17(2) (1980) 238–246. doi:10.1137/0717021
5. T. A. Grandine, F. W. Klein. A new approach to the surface intersection problem. *Computer Aided
   Geometric Design* 14(2) (1997) 111–134. doi:10.1016/S0167-8396(96)00024-6
6. C. M. Hoffmann. A dimensionality paradigm for surface interrogations. *Computer Aided Geometric
   Design* 7(6) (1990) 517–532. doi:10.1016/0167-8396(90)90013-H
7. G. A. Kriezis, N. M. Patrikalakis, F.-E. Wolter. Topological and differential-equation methods
   for surface intersections. *Computer-Aided Design* 24(1) (1992) 41–55.
   doi:10.1016/0010-4485(92)90090-W
8. J. Levin. A parametric algorithm for drawing pictures of solid objects composed of quadric
   surfaces. *Communications of the ACM* 19(10) (1976) 555–563. doi:10.1145/360349.360355
9. J. R. Miller, R. N. Goldman. Geometric algorithms for detecting and calculating all conic sections
   in the intersection of any two natural quadric surfaces. *Graphical Models and Image Processing*
   57(1) (1995) 55–66. doi:10.1006/gmip.1995.1006
10. R. E. Moore. A test for existence of solutions to nonlinear systems. *SIAM Journal on Numerical
    Analysis* 14(4) (1977) 611–615. doi:10.1137/0714040
11. N. M. Patrikalakis, T. Maekawa. *Shape Interrogation for Computer Aided Design and
    Manufacturing*. Springer (2002). doi:10.1007/978-3-642-04074-0
12. L. Piegl, W. Tiller. *The NURBS Book*, 2nd ed. Springer (1997). doi:10.1007/978-3-642-59223-2
13. T. W. Sederberg, R. J. Meyers. Loop detection in surface patch intersections. *Computer Aided
    Geometric Design* 5(2) (1988) 161–171. doi:10.1016/0167-8396(88)90029-5
14. T. W. Sederberg, T. Nishita. Curve intersection using Bézier clipping. *Computer-Aided Design*
    22(9) (1990) 538–549. doi:10.1016/0010-4485(90)90039-F
15. P. H. Sterbenz. *Floating-Point Computation*. Prentice-Hall (1974); reviewed in *SIAM Review*
    18(1) (1976), doi:10.1137/1018026.
16. J. Yang, X. Jia, D.-M. Yan. Topology guaranteed B-spline surface/surface intersection. *ACM
    Transactions on Graphics* 42(6), 211 (2023). doi:10.1145/3618349 (cited by #17)
