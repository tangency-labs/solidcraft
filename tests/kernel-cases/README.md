# Kernel regression rig

Every geometry case the old kernel (truck) got wrong, recorded in an issue, a pull request or a
crash report, is a use case here with its own right answer. The kernel, SMLib, is held to all of
them. `cargo xtask kernel-cases` runs them and writes [`docs/kernel-cases.md`](../../docs/kernel-cases.md).

```sh
cargo xtask kernel-cases                                    # every case
cargo xtask kernel-cases --cli target/release/solidcraft-cli --out /tmp/run   # with a CLI already built
cargo xtask kernel-cases --kernel both --cli target/release/solidcraft-cli  # SMLib and solidcore side by side
cargo run -p solidcraft-cli --features smlib -- kernel-cases tests/kernel-cases/booleans/<id>   # one case
```

The CLI links SMLib by default (`scripts/fetch-smlib.sh` fetches the prebuilt library first);
built with `--no-default-features`, every operation answers that SMLib is not built. The run's output folder (`--out`, default `target/kernel-cases`)
gets `results.json`, every case's full result, and `dumps/<area>-<case>/`: the inputs of each
SMLib call a case failed in, for replay (the CLI's `--dumps=DIR`, which the rig always passes).
`--validation MODE` runs the cases' features under that mode of SMLib's check
(`diagnostics.validation`): `each_feature` (the default: the bodies each feature keeps are checked
once), `each_step` (after every sweep, boolean and blend, as before) or `off`; the
`KNOWN_FAILING` lists are each_feature's.

`--kernel smlib|solidcore|both` (default `smlib`) picks the backend each case's process runs on,
through `SOLIDCRAFT_KERNEL`. A `solidcore` run keeps its `results.json`, `dumps/` and `report.md`
in `<out>/solidcore/` and leaves `docs/kernel-cases.md` alone. `both` runs every case on each
backend, keeps `docs/kernel-cases.md` as the SMLib report and writes the two side by side to
[`docs/kernel-cases-backends.md`](../../docs/kernel-cases-backends.md) (cases that differ first)
and `<out>/backends.json`.

`--route <op>=solidcore[,…]` (with `--kernel both` or `solidcore`) scores single operation
families: the solidcore side runs on SMLib (`SOLIDCRAFT_KERNEL=smlib`) with only the families
named moved to solidcore (`SOLIDCRAFT_KERNEL_ROUTES`), so an unported family (the body check, say)
doesn't hide the others' score. The smlib side stays plain SMLib. The report labels the side
`solidcore[boolean,measure]`, and its `results.json`, `dumps/`, `report.md`, `backends.json` and
`backends.md` go to `<out>/solidcore-boolean-measure/`, so runs don't overwrite each other. The
families are the lower-case names `diagnostics.kernel` prints (`file` stays on SMLib):

```sh
cargo xtask kernel-cases --kernel both --route boolean=solidcore --cli target/release/solidcraft-cli
```

## Rules

- **The answer never comes from a kernel.** Not from truck's output, nor any commercial
  kernel's. It comes from analysis or a hand calculation, or else from the invariants below.
- **No user designs.** A crash report or a repro design becomes a small synthetic recipe that
  shows the same geometry. Check for private content before committing anything.
- **No workaround on SolidCraft's side.** A case that fails stays failing until the kernel is
  fixed. SMLib defects go to tangency-labs/solidcore as issues (`priority: high`,
  `bug`), and the fix goes into the fork.

## Layout

One directory per area, one per case: `tests/kernel-cases/<area>/<id>/`.

| Area | Owner |
|---|---|
| `booleans` (combine, cuts, holes, patterns, split, mirror, draft) | booleans/shell/import/crash rig |
| `shell` (shell, offset, thicken) | booleans/shell/import/crash rig |
| `import` (STEP and IGES geometry) | booleans/shell/import/crash rig |
| `crashes` (crashes and hangs inside kernel operations) | booleans/shell/import/crash rig |
| `fillets`, `sweeps`, `threads`, `sheet-metal`, `tessellation` | history rig |
| `continuity` (designed seams of known order, G0 to G4, that prove the continuity measurement itself) | history rig |
| `solids` (cones, scale, align, bounding solids) | tool coverage |
| `frames` (frame members, inserted profiles) | tool coverage |
| `plastic` (boss, lip and groove, rest, snap fit, plastic rules) | tool coverage rig |
| `surfaces` (every `surface.*` tool: offset, ruled, untrim, unstitch, reverse normal, boundary, blend, match, fair, and a normal and an edge case of each of the others) | surfaces rig |
| `construction` (construction planes, axes and points: `construct.*`) | tool coverage rig |
| `inspect` (the inspect commands that return numbers) | tool coverage rig |
| `history` (the timeline's suppress, reorder, delete, roll back, edit and folders; configuration rows, columns and rules; parameters that drive geometry) | tool coverage rig |
| `assemblies` (joints and couplings, motion studies, contact, components and occurrences, interference and clearance between placed parts: placements and answers worked out by hand) | rig coverage |

Each area also has its gate files: `KNOWN_FAILING` and `PATHS` (see [the gate](#the-kernel-rig-gate)).

`<id>` is `<source number>-<short-name>`, such as `1894-prism-cut-touching-walls`. A case from a report with no issue or pull request (a failure the maintainers met in the app) is
`report-<short-name>` and lists no `sources`; its `what` says where it was reported. A case that
covers a command with no source at all is `cov-<short-name>`.

Each case has:

- `recipe.json`: an engine command script (`{"commands": [...]}`, as `solidcraft-cli run`
  takes) or an oracle recipe (`tests/oracle/*/recipe.json`). `$CASE` in a string is the case
  directory, for an input file such as `$CASE/input.step` (written by hand from the STEP or IGES
  specification, never a user's file), and `$OUT` a scratch directory outside the repository for
  files the case writes (an export).
- `expected.json`: the right answer, graded by `solidcraft-cli kernel-cases`.

A script may start with `"base": "<case directory>"` (relative to its own directory): that case's
commands, and its own base's, run first. The ladder's cases build on each other this way.

Each area also has `skipped.json` (or several, `skipped-<part>.json`, where an area is triaged in
parts): every issue and PR looked at that did not become a case, with one line each saying why.

```json
[{"source": "tangency-labs/solidcraft#123", "why": "UI only: the selection highlight, no geometry"}]
```

`sources.json` says what kind each source number is (an open or closed issue, a merged PR, one of
the frozen truck-geometry PRs, a `kernel-paused` PR), for the report's coverage by source.

## expected.json

```json
{
  "sources": ["tangency-labs/solidcraft#1894", "tangency-labs/solidcraft#1629"],
  "what": "a prism cut whose walls only touch the target's walls",
  "source": "analytic",
  "method": "box 40x30x20 minus the prism 10x30x5 flush with the side: V = 24000 - 1500",
  "body_count": 1,
  "volume_mm3": 22500.0,
  "area_mm2": 5200.0,
  "faces": 8
}
```

| Field | Meaning |
|---|---|
| `sources` | The issues, PRs or crash reports the case comes from (`owner/repo#N`). |
| `what` | One line: the geometry that went wrong. |
| `source` | `analytic` (numbers worked out by hand; `method` gives the formulas) or `invariants` (no number is known: only the outcome, validity and counts are graded). |
| `method` | How the numbers were derived, or which invariants hold and why. |
| `outcome` | `solid` (default: the script succeeds), `refused` (the right answer is a clean error, never a panic or a hang), or `solid_or_refused` (either a clean error or a result graded as usual, where the geometry has no single manifold answer, such as two bodies touching along a curve only). |
| `refusal` | Required with `outcome: refused` (optional with `solid_or_refused`): a part of the refusal's message, the reason it must be refused for. A refusal for another reason (an SMLib status, a different check) fails. |
| `valid` | Default `true`: every body is one closed shell (or `shells` in all) that passes the record's structure and geometry checks and SMLib's `check(All)`. `false` only where the result is legitimately not a solid (a sheet). Where SMLib can't take the record yet (a surface kind its conversion lacks), its check gives no verdict and is not graded. |
| `shells` | The total number of shells, where it isn't one per body: a body with a sealed void (2), several lumps kept in one body. |
| `body_count`, `faces`, `edges`, `vertices` | Exact counts, where the geometry fixes them. Faces and edges count whole analytic surfaces and curves once (a full cylinder side is one face). |
| `volume_mm3`, `area_mm2`, `center_of_mass`, `bbox` | As in `tests/oracle`: totals over every body, relative tolerance 1e-3 for volume and area, 1e-3 of the box diagonal for positions. |
| `faces`, `edges`, `vertices`, `shells`, `body_count` as `[min, max]` | An inclusive range, where the geometry only bounds a count (a modelled thread's faces). |
| `volume_mm3`, `area_mm2` as `[min, max]` | An inclusive range, where analysis only bounds the measure: a cut removes at most its tool's volume, a join adds at most the tool's. `method` derives both ends. |
| `continuity` | `{order, edges}`: every seam `edges` names reaches `order` (`g0` … `g4`), as Continuity Analysis (`inspect.continuity`) measures it: from the faces' exact surfaces, at its default 33 stations a seam and its default tolerances (1 µm, 1°, 0.01 / mm, 0.001 / mm², 0.0001 / mm³), never loosened per case. A failure names the worst seam (lowest order; of those, the one furthest past the next order's tolerance), its faces, the order it reaches and that next order's worst value. `edges`: `blend` (default), every seam of a face a fillet or chamfer made (a round, a chamfer face or a corner patch) with the faces either side of the edge it replaced, and every seam of a corner patch with the rounds it joins: what the blend is built to meet (not where a round runs into an end face, nor where two rounds meet without a patch: that seam is the input's, tangent only where the rounded edges were); `corner`, those of them on a corner patch (a blend made from several replaced edges); or a list of points `[[x,y,z], …]`, the edges nearest them, as Continuity Analysis picks them. `exact: true`: every seam reaches `order` and not the next (a case built to an order proves the measurement reads that order, no more and no less). A constant round meets its faces G1 (curvature jumps by 1 / r), a `curvature` round G2, a chamfer G0; a `corner: g2` patch is built G2 to the rounds and faces round it. |
| `continuity` (older form) | `{near, within_mm, clear_mm, curvature_jump_per_mm}`: over every tangent seam lying wholly within `within_mm` of `near` (a corner's patch and its neighbours), the curvature jump across it (the faces' normal curvatures square to the seam, from their exact surfaces, at 65 stations a seam) is at most `curvature_jump_per_mm` at every station at least `clear_mm` along the seam from a vertex where two of those seams turn (where a neighbour's own G1 seam ends, which no surface is G2 to both sides of). A rolling ball's corner jumps 1 / r everywhere. |
| `surface_types` | The set of kinds the faces' surfaces are, over every body, as the records hold them: `plane`, `cylinder`, `cone`, `sphere`, `torus`, `bspline` (a B-spline or NURBS surface) or `revolution` (a revolved curve). Exact: none missing, none extra. |
| `splines` | `[{degree: [p, q], spans?: [m, n]}, …]`: one entry per B-spline face, each matching one face (directions in either order); `spans`, the distinct knot intervals each way, only where the construction fixes them. |
| `boundary_edges` | The edges only one face uses (a sheet's free boundary), over every body, as the records hold them; exact or `[min, max]`. |
| `flat_pattern` | A sheet metal case's flat pattern, from the script's last `sheet.flat_pattern` result: `size_mm` ([w, h] of the outline), `area_mm2` (the outline's area less the cut-outs', relative tolerance as volume) and `bend_lines` (each `[[x, y], [x, y]]`, a bend's centre line in flat coordinates, matched one for one, either way round, and as many as the result has), positions within the box-diagonal tolerance. |
| `watertight` | `true`: each body's display mesh (chord tolerance `tolerance.mesh_mm`, default 0.01 mm) has no open and no over-shared edges, with vertices closer than `tolerance.weld_mm` (default 1e-4 mm) taken as one. |
| `timeout_s` | A longer timeout for this case than the run's (at most 1800 s), for cases that are slow by nature. A case that only needs it on a loaded machine shouldn't set it. |
| `tolerance` | Optional overrides: `rel`, `abs_mm`, `mesh_mm`, `weld_mm`. |
| `files` | The files the case wrote to `$OUT`, each `{path, json \| csv \| dxf}`, matched as a part of what is there: every key the case gives, an array item by item and as long, a whole number exactly, any other number within `rel`. `json`: the file's JSON; `csv`: its rows, as objects keyed by the header row; `dxf`: per layer of its ENTITIES section, how many of each entity (`{"CUT": {"CIRCLE": 1, "ELLIPSE": 0, "length_mm": 31.416}}`; a count may be a range `[min, max]`) and `length_mm`, the total length of its lines, arcs, circles, ellipses and polylines. |
| `results` | The commands' own answers, each `{step, pointer?, json, rel?, abs_mm?}`: the answer of the `step`th command (from 1, base commands included), or its part at the JSON `pointer` (RFC 6901), matched as `files` matches JSON: a whole number exactly (a count), another within `rel` (the case's relative tolerance unless given) or `abs_mm`, lists item by item. |
| `construction` | `{name: spec}`, a spec being `{point: [x,y,z]}`, `{axis: {through, dir}}` or `{plane: {through, normal}}`: each named construction point, axis or plane as the timeline evaluated it. A point within the position tolerance (`tolerance.abs_mm`, else 1e-3 of the box diagonal, of 1 mm where there is no body); an axis whose direction is parallel to `dir` (either way, sine of the angle at most 1e-6) and whose line passes within it of `through`; a plane likewise with `normal`. An axis or plane is graded as the line or plane it is, not by the origin it keeps. |
| `missing` | A capability SolidCraft has no command or parameter for yet (`"solid.thread has no start count"`): the case is reported **not supported yet** without running, so the gap stays visible. |

## The kernel rig gate

Every area is a regression suite: each case is one small case of one issue, with its exact
answer. `.github/workflows/kernel-rig.yml` runs them, and the ladder, on pull requests (not
drafts). A plan job (`scripts/ci/kernel_rig_plan.py`) picks the areas from the changed files, one
job builds `solidcraft-cli` (release, SMLib) and the rig's runner, and a matrix of shards, one per
selected area plus the ladder, runs them in parallel; the `kernel-rig` check passes when every
selected shard does (and when none was selected). Pushes to main and develop, merge groups, the
release gate (`release-gate.yml`) and a run by hand run every area. Each area's shard runs:

```sh
cargo xtask kernel-cases --area <area> --cli target/release/solidcraft-cli \
  --known-failing tests/kernel-cases/<area>/KNOWN_FAILING
```

`<area>/KNOWN_FAILING` lists the cases of that area that fail today, sorted, one a line with its
cause (`<case> <why it fails today>`; `#` starts a comment; the header names the baseline: the
develop commit and SMLib revision it was taken from). The gate fails on a **regression**, a
case that fails (or crashes, hangs, or is not supported yet) and is not listed, and on a
**fixed** case, one listed that passes. With `--known-failing` the run writes no report;
`docs/kernel-cases.md` stays the full run's.

**Updating a list.** The pull request that changes an outcome changes the list in the same pull
request, so the list only shrinks and never goes stale:

- a fix (yours, or an SMLib pin bump): remove the line of each case the shard reports as
  `FIXED`;
- a new case that fails today: add its line, sorted, with the cause from its result
  (`results.json` in the shard's `kernel-rig-results-<area>` artifact, or a local run);
- a regression: fix it in the same pull request.

To check a list locally before pushing: `cargo xtask kernel-cases --area <area> --cli
target/release/solidcraft-cli --known-failing tests/kernel-cases/<area>/KNOWN_FAILING`.

### Which areas a pull request runs

- `<area>/PATHS`: the source files whose change reruns that area, one git pathspec glob a line
  (`*` within a folder, `**` across folders; `#` starts a comment): the engine command modules,
  doc evaluation and kernel files its commands go through. Only the area's own files: never a
  file `PATHS-ALL` covers, nor a whole crate (the self-test fails on either).
- `PATHS-ALL`: the shared core, whose change reruns every area: SMLib (`crates/smlib/**`, its pin
  included) and `scripts/fetch-smlib.sh`, `crates/geom/**`, the kernel's core (bodies, extrude and
  revolve, booleans, the SMLib bridge, tessellation, what the grader measures), timeline
  evaluation, the solid feature commands, the grader, the runner and the workflow.
- A change under `<area>/` (a case, its `KNOWN_FAILING`, its `PATHS`) reruns that area.
- The ladder spans every operation: it runs whenever any area does, and on a change under
  `ladder/`.

The workflow's trigger paths are whole folders that hold every `PATHS` glob (a self-test,
`scripts/ci/test_kernel_rig_plan.py`, checks it), so a pull request that touches none of them
starts no job at all. To see what a change would run:
`git diff --name-only -z origin/develop... | python3 scripts/ci/kernel_rig_plan.py --paths -`.

**A new area** joins with its folder: its cases, a `KNOWN_FAILING` (empty but for its header
when every case passes), and a `PATHS` listing the files its commands go through. The plan finds
the areas from the folders, so the workflow needs no edit (unless a `PATHS` glob lies outside
the trigger paths, which the self-test reports). Add a row to the table under Layout.

### Every command has a case

`cargo xtask kernel-cases --coverage --cli target/release/solidcraft-cli` (run in the gate's
build job) lists every registry command (`solidcraft-cli commands`, planned ones aside) that no
`recipe.json` here runs, and fails unless `EXEMPT` lists it: one line a command, `<id>  <why it
has no case>`, each section sorted. Lines before `[to cover]` have no geometry to grade (the
view, rendering, appearance, files, component and parameter bookkeeping, ...). Lines after it
make geometry and wait for a case: that section only shrinks. The check also fails on a listed
command that a case now uses (remove its line in the same pull request) or that the registry no
longer has. A new geometry command gets a case, not an `EXEMPT` line. The build job runs for this
check on any change to `crates/engine/src/cmd/`, a recipe or `EXEMPT`, even when no area does.

## The complexity ladder

`ladder/` orders kernel operations by complexity instead of by where they went wrong: tiers from
primitives through degenerate booleans, analytic and spline profiles, holes and taps, threads,
fillets, chamfers, shells, patterns, whole parts, and sheet metal. Later tiers build on earlier tiers' shapes (a
running part grows one tier at a time through `base`), so the last case carries everything.

- `ladder/<NN-tier>/<NNN-name>/`: cases in the format above, each a test with its own answer.
- `ladder/ladder.json`: the tiers in order and, per tier, its cases in order (paths under
  `tests/kernel-cases/`, so a tier may also list a rig case from another area).
- `ladder/golden.json`: per case the status, the commands' time and what was made (counts,
  volume, area). Runs compare with it and flag slowdowns (over twice the time and
  0.25 s more) and changed counts or statuses. It is re-recorded only with a reason.
- `tools/kernel-ladder/cases.py` writes the recipes and answers (standard-library Python; each
  `method` gives the derivation).

```sh
cargo xtask kernel-cases --ladder --cli target/release/solidcraft-cli
cargo xtask kernel-cases --ladder --cli target/release/solidcraft-cli --gate
cargo xtask kernel-cases --ladder --cli target/release/solidcraft-cli --record "why"
```

The ladder needs no known-failing list: the golden is the reference. Its shard in the kernel rig
gate runs with `--gate`, which fails on any case whose status or counts differ from the golden,
better or worse, and on a case with no golden; times are not compared there (they are the
machine's). A pull request that changes a ladder result re-records the golden with
`--record "why the golden changes"` and commits `golden.json` and
`ladder.json` with it.

Ladder runs measure one case at a time (unless `--jobs`) and keep their output in
`target/kernel-ladder` (`--out`); `--record` rewrites the golden and sorts each tier by time,
fastest first; commit both files. The report, `docs/kernel-ladder.md`, gives per tier the counts
per status and the **frontier**: the first case in ladder order that fails.

## Results

Per case: **pass**; **fail** with the numbers; **not supported yet** (the kernel says
so, or the case names a `missing` capability); **crash** (a panic caught by the guards, or the process died); **hang** (no result before
the timeout).

## Exporting to solidcore

solidcore#349 proposes a graded case format for the Rust port. The cases here are SolidCraft's
copy and the source; each one maps onto it field by field (`sources`, the derivation, the
graded quantities and invariants), so an exporter can write them out for solidcore unchanged.
