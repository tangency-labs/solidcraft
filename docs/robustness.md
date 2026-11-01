# Kernel robustness

Real users hit edge cases the reference tests (docs/oracle.md) never see: faces that coincide
by accident, tangencies, seams lying where another body cuts, walls thinner than a shell. The
robustness runs build random but realistic designs and check every step.

## How it runs

```
solidcraft-cli fuzz --from 0 --count 1000 --jobs 8 --timeout 90 --out runs/ --report report.md
solidcraft-cli fuzz-one <seed> --minimise      # one design, cut down to the steps that matter
```

`--count` seeds are run starting at `--from`. A range that exceeds `u64::MAX` is
rejected before any jobs or output directories are created. The maximum seed
(`18446744073709551615`) is valid with `--count 1`; `--count 0` runs no seeds.

- **Designs** (`crates/engine/src/fuzz.rs`): a seeded generator starts from a box, an extruded
  sketch profile closed by an arc (sometimes notched), or a revolved section (partial or full
  turns, sometimes touching the axis). It then adds up to eight steps: joins and cuts of
  boxes, cylinders and spheres, fillets and chamfers on random edges, shells, holes (simple,
  blind, drilled), and rectangular and circular patterns of the last feature. Placements snap
  to a 2.5 mm grid and often to the body's own faces, so coincident faces and tangencies come
  up all the time, as they do in real parts.
- **Checks after every step:** the command succeeds; every body is a sound solid (each shell
  closed and consistently oriented, every face meshes, positive finite volume:
  `Body::validity`); the volume moves the right way (a cut never adds, a join never removes).
- **Boolean identity** (`crates/kernel/src/fuzz.rs`): for two random primitives on the same
  grid (boxes, cylinders, spheres, tori), union, intersection and cut each give a sound solid
  and V(A∪B) + V(A∩B) = V(A) + V(B), V(A−B) = V(A) − V(A∩B), within 0.2 %.
- Each seed runs in its own process with a time limit, so hangs and hard crashes are recorded
  too. A step the kernel refuses as **not supported yet** (a fillet between two curved faces,
  a shell thicker than a wall) is undone and counted separately, and the design goes on: those
  are known limitations, not failures. Booleans that fail count as failures even when their
  message says "not supported".
- `cargo test` runs a fixed set of seeds that must stay clean
  (`crates/engine/src/tests/robustness.rs`), next to the minimised cases fixed so far.

## Seeded goldens

`tests/robustness/goldens.json` records what each of seeds 0–95 builds (eight steps at most):
its steps, how it ended (ok, or the kind and the start of the error it was refused with), and
for the design it left the bodies' total volume and area (within a relative 1e-6) and their
face, edge and shell counts (exact). `tests::robustness_goldens` fails on any change, better or
worse, so a kernel change shows every seed it moves. Pull requests that can change them (the
kernel, engine, doc, sketch and geom crates) run every seed but the slowest few (`SLOW_SEEDS` in
`.github/workflows/ci.yml`, about four fifths of their time); the merge queue, pushes to `main`
and the nightly run them all. Each seed is its own test
(`seed_00` … `seed_95`), so the CI shards and their cores share them out.

Reference selection uses the compilation target's operating system and environment.
On Windows, `platforms.windows.seeds` contains the MSVC-verified records;
`platforms.windows-gnu.seeds`, when present, contains GNU records. An override
replaces the whole seed record. Seeds missing from the selected group use the shared
record, never another Windows group's record. Non-Windows targets keep their existing
OS-group selection and shared fallback. Unknown Windows environments compare with the
shared records and refuse recording until an explicit policy is defined.
All 96 seeds and the strict measurement and topology tolerances remain unchanged.

The Windows records for seeds 1, 23 and 34 were independently reproduced on unmodified
main `1e984c8e`; seed 94 was independently reproduced on unmodified main `c5992386`
after the extrusion rebase. Both match the extrusion branch. Seed 13's was recorded on
unmodified main `65e6d9da`; seed 23's was re-recorded after #1653. Seed 36's, recorded
on main `79a843c9`, holds a failure: on Windows (and macOS) its last step leaves a body that fails the
validity check, where Linux builds it ok (#3583). The record only makes a further change
show; it does not accept the failure. Recording on a platform with such entries updates its
changed seeds and preserves shared references. A platform record replaces the shared
one whole, so a change that moves a seed with a platform record needs that seed checked
on that platform too: seed 67's Windows record went stale this way after #2064 and
#2247 and was removed once Windows built the shared record again.
The Windows records were made with the MSVC target (`x86_64-pc-windows-msvc`) and all
six were checked again on main `e9490093`. GNU builds (`x86_64-pc-windows-gnu`)
use the shared records unless an explicit GNU override exists, so MSVC-only outcomes
do not leak into GNU validation (#4104).

To record an intended change, give the reason:

```
ROBUSTNESS_GOLDENS_BLESS="why it changed" cargo test --profile ci -p solidcraft-engine --lib robustness_goldens::seed_
```

The changed seeds and the reason are added to the file's `notes`; say why in the pull request too.
Recording on Windows always writes to the current environment's own group, even when
that group does not exist yet. It preserves shared records and the other Windows
environment's records. Non-Windows recording keeps its existing OS-group policy.
Notes coalesce only when both the reason and destination match.

The generator feeds the current geometry's bounds and face/edge picks back into later
commands. The same RNG seed can therefore generate different command coordinates or picks
on different math libraries. The original inputs are retained, including near-contact cases.
Platform snapshots track repeatability; they do not certify cross-platform geometric
equivalence or resolve the numerical differences tracked in #924.

### Independent measurement checks

The seeded snapshots also have checks against geometric references. Seed 34 starts with a
quarter cylinder of radius 15 and height 12.5 (`x,y >= 0`, `-12.5 <= z <= 0`), cuts it with a
radius-2.5 sphere centred at (10, 12.5, 0), then drills an independent radius-2, depth-5 blind
hole from (0, 7.5, -6.25) along +X, with a 118-degree tip.
Subtracting the sphere/cylinder intersection and the drill gives **2134.404133 mm³** and
**1086.316691 mm²**. The drill has tip height `2 / tan(59 degrees)`; the spherical bite is
integrated over the intersection of the radius-15 disk and the sphere's projected radius-2.5
disk. Integrating the lower spherical hemisphere and the removed cylinder-wall patch gives
the surface reference. The test compares the chordal measurement with these independent
references at 5e-5 relative error.

A fixed command replay recorded from seed 67 checks two disconnected shells kept in one
body after a cut (#2247), with a combined analytic surface inventory of 22 planar, six
cylindrical and one conical face after merging redundant subdivisions. The commands are
fixed because the generator chooses later operations from the current body and edge layout;
changes to body grouping can therefore change the generated design. This preserves the
original geometric reference independently of those choices. These checks supplement the
snapshots' stricter repeatability comparisons of raw topology and tessellated measurements.

## Successful Boolean retries

When an accepted Boolean result needs a retry that can make it differ from the
exact one (a tool nudge, or the unverified shortcut), the feature shows a
warning in the timeline. Every other retry works on temporary geometry or
tolerances and leaves the exact result, so it is recorded but not warned about.
`timeline.inspect` and `doc.inspect` expose every retry in the feature's
`boolean_diagnostics`, including the strategy and the applied translation,
rotation, face offset and tessellation tolerance in millimetres/radians.
Candidates those retries discarded before measuring them are listed in its
`rejected`, with the reason: a candidate with an edge further outside the
operands than 5% of their size cannot be their union, cut or intersection,
and meshing one (a trim loop running metres along a cone's continuation)
could take minutes or never finish (#3020).

A common shift of both operands is reversed on the result. Coincident-face
separation and bore offsets describe temporary working geometry; they are not
claims about the final model's error. A coincident face is separated only
where the space it moves through holds what the Boolean leaves there anyway
(nothing of the target for a cut's face, the target's material for a join's);
a face with no such room stays, and the exact planar fallback is tried first.
A tool nudge changes the relative operand
placement and reports both its translation magnitude and rotation angle. When it
fails, or leaves a sliver face (narrower than 1/200 of the part), it is tried the
other way too: one way can push a face into a curved face it only touched (a cut
whose end lies tangent to a round side) and leave a sliver of it, where the other
way parts them (#897, #827).
Exact planar and piecewise fallbacks report zero displacement. When a conical
cutter meets a body with earlier conical faces (pointed bores copied in a pattern),
two finer, unshifted passes are tried first, at 1e-5 and 1e-6 of their size (never
below 1e-6 mm). A coarse first cut can leave tiny junctions that later copies cannot
intersect, so refining only the failing copy is too late. Every candidate must pass
the volume, membership and spatial checks; a rejected candidate continues to the
next tolerance, then the ordinary routes. The existing acceptance checks still
apply to all results.

A join or a cut that needed any retry is then held to what it can do at all
(`crates/kernel/src/boolean_guard.rs`, #616): the result is closed, a join
never loses volume and a cut never gains any, and outside the tool's box the
result's surface is the target's and the target's is the result's (a join or
a cut can only change the target where the tool is). A cut may come out empty
only when the tool covers the target. A result that fails is refused with an
error naming what was wrong and where, rather than returned: a wrong result
that looks right is worse than an error.

Reports belong to the evaluated feature and survive cached rebuilds and
undo/redo. Cold reopening regenerates them from the embedded design. Failed or
discarded retry branches do not contribute successful-retry events. Reports
are capped at 1024 events per feature and explicitly identify truncation.

Fuzz JSON and the text report count successful retry events by strategy for
designs and Boolean identity checks separately, including nested operations.
These are event counts, so a single Boolean or seed can contribute several.
Truncated reports are counted separately rather than implying complete totals.

## Near contact and the retry budget

Bodies that only just meet (an arm whose round end reaches a hundredth of a millimetre into a
ring's rounded rim, or stops that far short of it) are where the meshed shortcuts and the
intersection both go wrong. Before any intersection, bodies whose samples show no shared
volume are looked at on their exact faces (`crates/kernel/src/distance.rs`, `contact`): near
every place their meshes come within both chords of each other, the nearest points of the faces
and their edges are found by least squares on the gap.

- **Apart**: every gap is clear of round-off. The bodies share nothing whatever their meshes
  say: a union keeps both side by side, an intersection is empty, a cut leaves the target, at
  once and with no retry. (A 0.01 mm gap took most of a minute and was reported "unchecked".)
- **Crossing**: two faces cross at an angle inside both. They share volume the samples were too
  coarse to see: the shortcut is not taken, a union that keeps both shells is refused, an
  empty intersection is not believed, and the intersection is traced at steps finer than the
  usual ones, down to a fraction of how far the faces could overlap.
- **Touching**: anything else; the boolean goes on as before.

The membership check on a result skips sample points within two chords of any of the three
meshes: there two facetings of one face can disagree although the faces agree, and where the
bodies only just overlap, the overlap box is no thicker than that (a correct join of an arm
0.1 mm into a rounded rim was refused for it, after minutes of retries).

Each boolean makes at most 64 attempts at the intersection, its retries included (shifted and
refined tries, nudges, and the booleans those make in turn). Every nudge used to run the whole
round of retries again, so a join that could not succeed took minutes to say so; once the
attempts are spent the rest is skipped and the error says so. It is a count, not a clock, so a
design rebuilds the same way on any machine. A join of faces lying on one cylinder, back to
back, that no route can take says so in its error rather than leaving the bodies unjoined by a
nudge that only moved them apart.

## Results

1000 seeds (0–999), 8 processes on a heavily loaded machine (load average 60–80), 90 s per
seed. A "timeout" there is mostly a design whose booleans are very slow (seed 20 finishes in
2 min 8 s alone), not a hang. The "after" run had more of them: the machine was busier still
(test suites ran alongside), and failing booleans now try more fallbacks before giving up;
fewer designs fail outright (27.1 % against 36.3 %). Both figures are from before the last two
fixes below (sound face pushes, no panics in the boolean's final assembly).

| Designs | before | after |
|---|---|---|
| clean | 49.8 % | 53.8 % |
| failed | 36.3 % | 27.1 % |
| invalid body | 0.7 % | 0.9 % |
| volume the wrong way | 0.2 % | 0.2 % |
| over 90 s | 13.0 % | 18.0 % |

| Boolean identity | before | after |
|---|---|---|
| holds | 85.4 % | 80.8 % |
| a boolean failed | 1.6 % | 1.2 % |
| not run (the design ran out of time) | 13.0 % | 18.0 % |

### Fixed

- Back-to-back contact joins preserve intentional Split Face boundaries, including on
  faces away from the contact. The glue pass retains their lines during healing and in
  the resulting body for subsequent operations. Partial circular-contact regressions
  cover offsets, rotation, operand order, volume, area and history; the seed-16 pattern
  also runs in the ordinary engine suite with analytic volume and area checks.
  Imprinting visits a shared analytic support only once: the two side faces of a
  cylinder must not recut the same outline into redundant arcs.

- **Shell corners where a curved face meets another at an angle** (the biggest cause, 215 of
  the 363 failed designs before). Moving a vertex took one linear step, exact only for planes;
  a cylinder meeting a plane at an angle (a shallow arc side) landed off the moved surface.
  Now refined by Newton steps onto the moved surfaces.
- **Shell edges that are neither lines nor arcs** (a hole's wall meeting a curved side): the
  edge is rebuilt through its moved points.
- **Shells of bodies in several pieces** ("this shell is not connected"): one shell per piece.
- **Cylinder seams on the grid.** A cylinder's seam lines lay on its frame's axes, exactly
  where faces placed on the same grid cut it, and booleans failed along the seam. Seams now
  start at an odd angle: cylinders tangent to box faces from inside or out, and crossing
  cylinders touching along a line, now work.
- **Partial revolves of profiles touching the axis** (a quarter or half turn of a rectangle
  from the axis): the edge on the axis no longer sweeps a zero-area face, which every boolean
  attempt failed on.
- **Booleans on bodies in several pieces** (a join that left two lumps): when the whole fails
  or comes out empty, a piece at a time.
- **Pushing a face apart** (coincident faces) checks its result: the exact rebuild could leave a
  cylinder unsound, and then the general offset is used. A boss standing on a plate joins again.
- **No panics in the boolean's final assembly.** Natively a panic inside the kernel is caught,
  but the web build aborts on any panic; the Plastic Enclosure sample panicked 8 times building
  its bosses and crashed the start page in the browser. The vendored shapeops now returns
  `None` instead, and a test builds every sample with a panic counter. The runs count caught
  panics per design ("Caught panics" in the report), since each one would crash the web build.

### The first tutorial (box, fillets, shell)

Found by QA on new users' first steps, each with an engine test against the analytic volume
(`crates/engine/src/tests/robustness.rs`):

- **Shell of a filleted box** (vertical edges r5, bottom loop r3, 2 mm, top open): the bottom
  round runs round the corners as spindle tori (the tube wider than its circle), which the
  offset now takes. Volume within 1e-5 of the analytic one.
- **Shell thicker than or equal to a round** (r2 rounds shelled 2 mm): the round's inside
  collapses to a sharp edge, as intended, instead of "a cylinder shrinks to nothing".
- **Shell thinner than a round tangent to the open face** (1.5 mm): every face moves in
  (tangencies keep) and the opening is the open face's copy swept out through the body.
- **Shell after a drilled hole** (its tip a cone).
- **Filleting a box's edges one at a time**: an edge that runs into an earlier round takes
  that round off and rounds both together, with the corner where they meet (`blend_corner.rs`,
  any radii).

### Speed

- Measuring a body of 5000 faces (a modelled thread): 13 s to 0.8 s.

### Known limitations (counted as "not supported", with a clear message)

- A shell thicker than a wall or a step: the moved faces would cross and the cavity turn
  inside out. The intended behaviour fills such places solid; we need a topology-changing offset.
- Fillets and chamfers between two curved faces, or ending at faces that aren't planar.

### Still failing

- Partial unions of a cap and a face (a cylinder's end lying partly on a box face): pushing
  one face into the other adds material where they don't overlap.
- Shells of partial annuli (a half ring) whose cavity truck's boolean rejects although the
  same cavity built another way cuts cleanly.
- Very slow booleans on bodies with many patterned features.
- Joins where faces of both bodies lie on one cylinder back to back and the rest of them meets
  it tangentially (a concave arm end hugging a ring with rounded rims): refused, with a reason,
  after its attempts.
