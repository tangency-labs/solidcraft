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
`scripts/fetch-smlib.sh`). The rig sets `SOLIDCRAFT_BOOLEANS=smlib` for those runs.

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

Each area also has `skipped.json` (or several, `skipped-<part>.json`, where an area is triaged in
parts): every issue and PR looked at that did not become a case, with one line each saying why.

```json
[{"source": "tangency-labs/solidcraft#123", "why": "UI only: the selection highlight, no geometry"}]
```

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
| `outcome` | `solid` (default: the script succeeds) or `refused` (the right answer is a clean error, never a panic or a hang). |
| `valid` | Default `true`: every body is one closed shell (or `shells` in all) that passes the record's structure and geometry checks and, in builds with SMLib, SMLib's `check(All)`. `false` only where the result is legitimately not a solid (a sheet). Where SMLib can't take the record yet (a surface kind its conversion lacks), its check gives no verdict and is not graded. |
| `shells` | The total number of shells, where it isn't one per body: a body with a sealed void (2), several lumps kept in one body. |
| `body_count`, `faces`, `edges`, `vertices` | Exact counts, where the geometry fixes them. Faces and edges count whole analytic surfaces and curves once (a full cylinder side is one face). |
| `volume_mm3`, `area_mm2`, `center_of_mass`, `bbox` | As in `tests/oracle`: totals over every body, relative tolerance 1e-3 for volume and area, 1e-3 of the box diagonal for positions. |
| `tolerance` | Optional overrides: `rel`, `abs_mm`. |

## Results

Per case and kernel: **pass**; **fail** with the numbers; **not supported yet** (the kernel says
so); **crash** (a panic caught by the guards, or the process died); **hang** (no result before
the timeout).

## Exporting to solidcore

solidcore#349 proposes a graded case format for the Rust port. The cases here are SolidCraft's
copy and the source; each one maps onto it field by field (`sources`, the derivation, the
graded quantities and invariants), so an exporter can write them out for solidcore unchanged.
