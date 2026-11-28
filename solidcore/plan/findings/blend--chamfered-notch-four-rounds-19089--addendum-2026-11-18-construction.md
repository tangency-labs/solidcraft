# blend: the missing chamfered-notch corner event (2026-11-18)

Updates: `blend--chamfered-notch-four-rounds-19089.md`.

Issue: tangency-labs/solidcraft#19089. This addendum distinguishes a valid local
rolling-ball construction from the legacy corner construction that cannot make it.

## A local exact construction exists

Keep the original notation: the notch cylinder has radius `R = 3 sqrt(2)` and
vertical axis through `(R,R)`, and the new rounds have radius `r=2`. Write
`q=(sqrt(2)-1)r`. A ball touching the bottom `z=0`, the chamfer `y=z`, and the
cylinder has centre

```text
C = (R-s, -q, r)
s = sqrt((R+r)^2 - (R+q)^2)
```

The first two contacts impose `z_c=r` and `y_c=-q`, as in the original finding.
The cylinder contact imposes `(x_c-R)^2+(y_c-R)^2=(R+r)^2`. The minus square-root
branch is on the reported notch's chamfer. For this input,
`s^2=32(sqrt(2)-1)` and `C` is approximately
`(0.6019218026214661,-0.8284271247461903,2)`.

The contacts are, without fitting or a new tolerance:

```text
bottom:   (R-s, -q, 0)
chamfer:  (R-s, -q+r/sqrt(2), r-r/sqrt(2))
cylinder: (R-R*s/(R+r), R-R*(R+q)/(R+r), r)
```

At the reported radius these lie inside the original trimmed faces. In particular,
the cylinder contact is approximately `(1.7683253,0.7962276,2)`, above the chamfer
cut `z=y`. The chamfer contact is approximately `(0.6019218,0.5857864,0.5857864)`.
These approximations are explanatory only, not kernel parameters or test goldens.

The additional bottom/cylinder round's ball centres follow the circle
`(x-R)^2+(y-R)^2=(R+r)^2`, `z=r`, from `(R,-r,r)` to `C`. Its envelope is a torus
piece. It joins the existing bottom/wall round tangentially at the cylinder/wall
seam, and meets the bottom/chamfer and chamfer/cylinder rounds at a three-support
spherical corner. The centre circle's arc stays on `-r <= y <= -q`; its balls
remain on the material sides of both planes and outside the notch cylinder.

This establishes the local constituent surfaces. It does not certify a completed
globally trimmed B-rep. The parallel-rail obstruction therefore does not prove
the requested fillet impossible.

## The missing representation

The saved synthetic topology identifies three selected edges at `(R,0,0)`:
chamfer/cylinder, bottom/chamfer, and bottom/wall. The fourth edge is the
unselected vertical wall/cylinder seam. The two selected bottom edges are adjacent
in the bottom face sector, so `FourByThree` really does request the parallel-rail
intersection identified in the original finding.

This rolling-ball construction needs a new bottom/cylinder solver geometry,
although those two faces share only a vertex in the input body. Its terminal
section must be owned by the same Ball as the other two rounds. Current
interfaces do not represent that composition:

- `fillet_corner/topology/ball.rs:59-156` constructs Ball topology using original
  target edgeuses, contact faces from that ring, and each solver's first geometry.
- `blend_corner/ball.rs:564` reconstructs the contact faces from the same original
  ring and starts from contacts that the corner vertex stage has already solved.
- `fillet_solver/p5/recovery.rs:546` repairs a retraced End with an end cap and
  `RailXExtendedEdgeuse` vertices; it does not attach a new segment to shared Ball
  contacts.

The generic blend's Ball and smooth-support continuation constructions supply the
geometry. They still need a general event topology that gives a terminal rollover
segment and its shared Ball explicit geometry ownership independent of the target
edge ring. The current reference design does not settle whether that belongs in
the pretrace event graph or expanded posttrace recovery. Changing only the
classification, postponing the first end-cap check, or inventing a different
rail-intersection point cannot supply the missing topology.

No experimental production patch is retained. The success-expectation regression
is retained in the existing `solidcore-ops/tests/blend.rs`, ignored with the issue
number while the construction is missing. Its exact edge picks use the
cylinder/chamfer point `x=y=z=R-3`, since `(-3)^2+(-3)^2=R^2`. It requires that no
selected edge is dropped, that no holes remain, and that the result passes the
full kernel check.

## The existing Setback is not a different ready construction

Removing the Setback classifier's all-selected and nontangent-edge guards would
not supply a fill. `blend_corner/setback.rs:841` (`curved_least`) sizes the hole
from adjacent rail intersections. The bottom rails have no such intersection.
Even with explicit setbacks, their endpoint tangents are still parallel, whereas
`blend_setback/spring.rs:91-95` requires a nonzero determinant to make the spring
curve (`two rails on a face run parallel`).

The fill also requires one support face per round
(`blend_setback.rs:2275`). Its `FaceKind` represents a plane, cylinder, cone, or
sphere (`blend_setback.rs:814`), rather than a smooth plane/cylinder chain. This
corner has four support faces and three selected rounds. Treating the wall and
cylinder as one current support would not be exact: a single rational Bezier
span lying in the wall plane over an interval lies in it identically, so it
cannot leave the plane to follow the cylinder. A composite boundary needs
segmentation at the seam and the corresponding piecewise normal field.

A generalized Setback remains possible. It needs new parallel-tangent spring
geometry, sizing, support segmentation, and compatible topology and certificate.
The current curved fill is not a smaller fix hidden behind classification guards.

## Existing sequencing does not supply that event

Diagnostic CLI calls on the synthetic input also tried each of the four picked
edges alone, the two bottom edges together, and the line/ellipse chain together.
With tangent propagation enabled, all six calls were refused. With propagation
disabled, the positive-x bottom/wall edge alone built; the other five calls were
refused. These are diagnostic experiments, not a proposed selection-order recipe.

The successful call produces a horizontal radius-2 cylinder and a planar end cap
at `x=R`. It produces no torus. The two remaining edges at the original corner
are adjacent selected edges in a four-edge ring; the current Nx2 topology does not
accept that arrangement (`fillet_corner/topology.rs:904-910`). Applying the
remaining original selections cannot simply reuse a Ball event. A multi-pass
event would still need to introduce the bottom/cylinder support and replace the
end cap explicitly.

## Validation after the worker temporary-file repair

Commands ran from the worktree root. Builds use the requested local temporary
directory and two compile jobs:

```text
TMPDIR=/home/ray/builds/lane-19089/tmp buildq run solidcraft . -- cargo --config 'env.TMPDIR.value="/home/ray/builds/lane-19089/tmp"' --config env.TMPDIR.force=true clippy --jobs 2 --manifest-path solidcore/Cargo.toml -p solidcore -p solidcore-ops --all-targets -- -D warnings
PASS, exit 0. Buildq job 1795024162.580041-8ddad940; 109 seconds.

TMPDIR=/home/ray/builds/lane-19089/tmp buildq run solidcraft . -- cargo fmt --all
PASS, exit 0.

TMPDIR=/home/ray/builds/lane-19089/tmp buildq run solidcraft . -- cargo fmt --manifest-path solidcore/Cargo.toml --all
PASS, exit 0.

TMPDIR=/home/ray/builds/lane-19089/tmp solidcraft-cli run /home/ray/builds/lane-19089/tmp/notch.json --out /home/ray/builds/lane-19089/tmp/notch.sc
PASS, exit 0: one closed body, 11 faces, 25 edges.

TMPDIR=/home/ray/builds/lane-19089/tmp solidcraft-cli run /home/ray/builds/lane-19089/tmp/rounds.json --in /home/ray/builds/lane-19089/tmp/notch.sc
FAIL, exit 1: status 1001, Fillet Corner Error: Unable to Make Topology.
```

The installed CLI reports version 0.44.1; the Rust regression below builds the
current worktree. The two CLI scripts are the original finding's primitive-only
scripts. Additional checks passed: reproduction JSON parsing, whitespace checks
on both new text files, the commit-message restrictions, and `git diff --check`.

The restored regression was run unignored as part of its existing test file:

```text
TMPDIR=/home/ray/builds/lane-19089/tmp buildq run solidcraft . -- cargo --config 'env.TMPDIR.value="/home/ray/builds/lane-19089/tmp"' --config env.TMPDIR.force=true test --jobs 2 --manifest-path solidcore/Cargo.toml -p solidcore-ops --test ops blend:: -- --nocapture --test-threads 2
```

`blend::rounds_meet_a_chord_round_at_a_chamfered_notch` failed at the final round
call with `BlendFailed { class: Failed, status: 1001, phrase: "the fillet body is
not built: Fillet Corner Error: Unable to Make Topology" }`. This is a runtime
geometry failure after successful compilation on the repaired XPS worker, not a
temporary-file or cargo setup failure. The test's expected success was then
marked ignored; refusal is not asserted as correct behavior.

Final result: **66 passed, 1 failed, 0 ignored, 391 filtered out**, exit 101.
The test process took 3030.63 seconds; buildq reported 3368 seconds for job
`1795027615.202139-f853bc5b`. All 63 existing tests from `blend.rs` passed,
including `the_inner_top_outline_of_13911_chamfers_and_rounds_at_every_size`;
the new regression was the only failure. The retained ignore attribute was
added after this run's source snapshot, so the reported run exercised the
regression body rather than skipping it.

Rust's substring filter also selected three `curve_blend` tests, all of which
passed. To limit a future area run to `blend.rs`, add `--skip curve_blend::`
after `--`; the exact regression command below does not select them.

To replay the retained known failure explicitly, add `--ignored`:

```text
TMPDIR=/home/ray/builds/lane-19089/tmp buildq run solidcraft . -- cargo --config 'env.TMPDIR.value="/home/ray/builds/lane-19089/tmp"' --config env.TMPDIR.force=true test --jobs 2 --manifest-path solidcore/Cargo.toml -p solidcore-ops --test ops blend::rounds_meet_a_chord_round_at_a_chamfered_notch -- --ignored --exact --nocapture
```

Initial command setup did not reach Rust: cargo rejected an inline-table
`--config` form (exit 101); it was replaced with the separate value/force settings
above. Queued replacements were cancelled before execution to apply the kernel's
compile-job limit and combine the regression with its existing test file (exit
130, not validation results). Two initial fmt calls were refused while two jobs
were waiting (exit 2); the fmt retries above passed. No workspace test suite,
`cargo xtask ci`, golden blessing, commit, or push ran.

## Built: the support change corner (2026-11-27)

The construction above is now a blend corner event of its own,
`BlendCornerEvent::SupportBall` (`blend/blend_corner/support_ball.rs`). A 4x3
vertex is classified as one where its one edge not blended is a straight seam
along which a plane W runs tangent into a cylinder Y, the face G beside W is a
plane square to Y's axis, the face H past Y is a plane, all three rounds are
equal constant convex rounds, and the ball on G and H reaches Y on Y's side of
the seam. The corner keeps the solvers as they are and owns two exact faces:

- the torus about Y's axis from the bottom/wall round's section through the
  seam's line to the ball C touching G, H and Y (a `Torus` keeping the Swept
  turn's definition: the section revolved about the axis, on G);
- the sphere's patch on C's ball between its contacts (the Ball's patch and
  definition).

Its topology: `Pc` (the two rounds' rails crossing on H), `Pb` and `Py` (their
sections' other ends on G and Y, placed in closed form), `Vs` (the wall rail's
crossing with the seam) and its mate `Vb`; three end sections, the torus's two
contacts on G and Y as precomputed edges, and the arc the two faces share.
