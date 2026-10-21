# Kernel regression rig

Every geometry case the old kernel (truck) got wrong, recorded in an issue, a pull request or a
crash report, is a use case here with its own right answer. The rig holds every kernel to all of
them. `cargo xtask kernel-cases` runs them and writes [`docs/kernel-cases.md`](../../docs/kernel-cases.md).

```sh
cargo xtask kernel-cases                        # the default kernel
cargo xtask kernel-cases --kernel both --cli target/release/solidcraft-cli   # and SMLib
cargo run -p solidcraft-cli -- kernel-cases tests/kernel-cases/booleans/<id>     # one case
```

For the `smlib` column, build `solidcraft-cli` with `--features smlib` (after
`scripts/fetch-smlib.sh`). The rig sets `SOLIDCRAFT_KERNEL=smlib` for those runs.

## Rules

- **The answer never comes from a kernel.** Not from truck's output, nor any commercial
  kernel's. It comes from analysis or a hand calculation, or else from the invariants below.
- **Kernels are graded against the case, never against each other.**
- **No user designs.** A crash report or a repro design becomes a small synthetic recipe that
  shows the same geometry. Check for private content before committing anything.
- **No workaround on SolidCraft's side.** A case that fails on a kernel stays failing until that
  kernel is fixed. SMLib defects go to tangency-labs/solidcore as issues (`priority: high`,
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

`<id>` is `<source number>-<short-name>`, such as `1894-prism-cut-touching-walls`.

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
| `valid` | Default `true`: every body is one closed shell (or `shells` in all) that passes the record's structure and geometry checks and, in builds with SMLib, SMLib's `check(All)`. `false` only where the result is legitimately not a solid (a sheet). Where SMLib can't take the record yet (a surface kind its conversion lacks), its check gives no verdict and is not graded. |
| `shells` | The total number of shells, where it isn't one per body: a body with a sealed void (2), several lumps kept in one body. |
| `body_count`, `faces`, `edges`, `vertices` | Exact counts, where the geometry fixes them. Faces and edges count whole analytic surfaces and curves once (a full cylinder side is one face). |
| `volume_mm3`, `area_mm2`, `center_of_mass`, `bbox` | As in `tests/oracle`: totals over every body, relative tolerance 1e-3 for volume and area, 1e-3 of the box diagonal for positions. |
| `faces`, `edges`, `vertices`, `shells`, `body_count` as `[min, max]` | An inclusive range, where the geometry only bounds a count (a modelled thread's faces). |
| `watertight` | `true`: each body's display mesh (chord tolerance `tolerance.mesh_mm`, default 0.01 mm) has no open and no over-shared edges, with vertices closer than `tolerance.weld_mm` (default 1e-4 mm) taken as one. |
| `timeout_s` | A longer timeout for this case than the run's (at most 1800 s), for cases that are slow by nature. A case that only needs it on a loaded machine shouldn't set it. |
| `tolerance` | Optional overrides: `rel`, `abs_mm`, `mesh_mm`, `weld_mm`. |
| `missing` | A capability SolidCraft has no command or parameter for yet (`"solid.thread has no start count"`): the case is reported **not supported yet** without running, so the gap stays visible. |

## The complexity ladder

`ladder/` orders kernel operations by complexity instead of by where they went wrong: tiers from
primitives through degenerate booleans, analytic and spline profiles, holes and taps, threads,
fillets, chamfers, shells, patterns, to whole parts. Later tiers build on earlier tiers' shapes (a
running part grows one tier at a time through `base`), so the last case carries everything.

- `ladder/<NN-tier>/<NNN-name>/`: cases in the format above, each a test with its own answer.
- `ladder/ladder.json`: the tiers in order and, per tier, its cases in order (paths under
  `tests/kernel-cases/`, so a tier may also list a rig case from another area).
- `ladder/golden.json`: per case and kernel the status, the commands' time and what the kernel
  made (counts, volume, area). Runs compare with it and flag slowdowns (over twice the time and
  0.25 s more) and changed counts or statuses. It is re-recorded only with a reason.
- `tools/kernel-ladder/cases.py` writes the recipes and answers (standard-library Python; each
  `method` gives the derivation).

```sh
cargo xtask kernel-cases --ladder --kernel both --cli target/release/solidcraft-cli
cargo xtask kernel-cases --ladder --kernel both --cli target/release/solidcraft-cli --record "why"
```

Ladder runs measure one case at a time (unless `--jobs`); `--record` rewrites the golden and sorts each tier by the first
kernel's time, fastest first; commit both files. The report, `docs/kernel-ladder.md`, gives per
tier and kernel the counts per status and the **frontier**: the first case in ladder order the
kernel fails.

## Results

Per case and kernel: **pass**; **fail** with the numbers; **not supported yet** (the kernel says
so, or the case names a `missing` capability); **crash** (a panic caught by the guards, or the process died); **hang** (no result before
the timeout).

## Exporting to solidcore

solidcore#349 proposes a graded case format for the Rust port. The cases here are SolidCraft's
copy and the source; each one maps onto it field by field (`sources`, the derivation, the
graded quantities and invariants), so an exporter can write them out for solidcore unchanged.
